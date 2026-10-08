package cost

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5/pgconn"
	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog/log"
)

const (
	// handlerTimeout bounds one delivery's verification and write. It is well inside
	// ackWait, so a delivery that is still running is never redelivered under itself.
	handlerTimeout = 30 * time.Second
	ackWait        = 60 * time.Second

	// maxAckPending bounds how many unacknowledged deliveries the broker hands out. The
	// subscription callback runs one delivery at a time, so this is only ever a ceiling.
	maxAckPending = 512

	// maxRetryDelay caps the pause before a failed delivery is tried again: a database that
	// is down for a minute is retried every half minute, not every two seconds, and never
	// given up on.
	maxRetryDelay = 30 * time.Second
)

// retryStep is the pause added per failed attempt. A variable so the durability tests do not
// wait in seconds.
var retryStep = 2 * time.Second

// delivery is one message of the durable stream. It is a seam so the decisions below are
// tested without a broker; the broker itself is exercised by the durability tests.
type delivery interface {
	Payload() []byte
	// Attempts is how many times the broker has delivered this message, this one included.
	Attempts() (uint64, error)
	// Sequence is the message's place in the stream, for finding it again.
	Sequence() uint64
	Ack() error
	NakWithDelay(time.Duration) error
	Term() error
}

type natsDelivery struct{ message *nats.Msg }

func (d natsDelivery) Payload() []byte { return d.message.Data }

func (d natsDelivery) Attempts() (uint64, error) {
	metadata, err := d.message.Metadata()
	if err != nil {
		return 0, err
	}
	return metadata.NumDelivered, nil
}

func (d natsDelivery) Sequence() uint64 {
	metadata, err := d.message.Metadata()
	if err != nil {
		return 0
	}
	return metadata.Sequence.Stream
}

func (d natsDelivery) Ack() error                             { return d.message.Ack() }
func (d natsDelivery) NakWithDelay(delay time.Duration) error { return d.message.NakWithDelay(delay) }
func (d natsDelivery) Term() error                            { return d.message.Term() }

// outcome is what became of one delivery.
type outcome string

const (
	// outcomePersisted: verified and written; acknowledged.
	outcomePersisted outcome = "persisted"
	// outcomeDuplicate: this process had already written it (the broker redelivered after a
	// lost acknowledgement); acknowledged, nothing written twice.
	outcomeDuplicate outcome = "duplicate"
	// outcomeInvalid: not an authentic event of a known producer. No retry can make it one:
	// terminated, and kept in the stream (limits retention) for seven days so it can be
	// looked at.
	outcomeInvalid outcome = "invalid"
	// outcomeTransient: the write failed for a reason that is not the event's (a database
	// that is down, a replay cache that is full). Not acknowledged, tried again later, for
	// as long as it takes: an event of real spend is never given up on.
	outcomeTransient outcome = "transient_failure"
	// outcomePermanent: the event is authentic and the database refused it for what is in
	// it (a value out of range). Terminated, kept in the stream like an invalid one.
	outcomePermanent outcome = "permanent_failure"
)

// handleDelivery decides what to do with one delivery and does it. It acknowledges only
// after the row is written, which is the whole point of the durable consumer.
//
// Nothing about the event but its position in the stream reaches the log: not the model, not
// the organization, not a payload byte. A poison message is found again by its sequence.
func (c *Consumer) handleDelivery(ctx context.Context, d delivery) outcome {
	attempts, err := d.Attempts()
	if err != nil || attempts == 0 {
		attempts = 1
	}
	verified, err := c.handle(ctx, d.Payload(), true)
	switch {
	case err == nil:
		if verified.Stale {
			staleAcceptedTotal.Inc()
		}
		return c.settle(d, outcomePersisted, d.Ack())
	case errors.Is(err, errReplay):
		return c.settle(d, outcomeDuplicate, d.Ack())
	case errors.Is(err, errInvalidEnvelope):
		log.Warn().Uint64("stream_sequence", d.Sequence()).Msg("cost ledger: event refused as not authentic; terminated")
		return c.settle(d, outcomeInvalid, d.Term())
	case isPermanentStoreError(err):
		log.Error().Err(err).Uint64("stream_sequence", d.Sequence()).Msg("cost ledger: the database refused an authentic event; terminated")
		return c.settle(d, outcomePermanent, d.Term())
	default:
		delay := min(time.Duration(attempts)*retryStep, maxRetryDelay)
		log.Warn().Err(err).Uint64("stream_sequence", d.Sequence()).Uint64("attempt", attempts).Dur("retry_in", delay).
			Msg("cost ledger: event not recorded; will be delivered again")
		return c.settle(d, outcomeTransient, d.NakWithDelay(delay))
	}
}

// settle counts the outcome and reports a failure to tell the broker. A failed
// acknowledgement is harmless (the broker redelivers and the row is already there), so it
// is logged, not escalated.
func (c *Consumer) settle(d delivery, result outcome, ackErr error) outcome {
	consumedTotal.WithLabelValues(string(result)).Inc()
	if ackErr != nil {
		log.Warn().Err(ackErr).Uint64("stream_sequence", d.Sequence()).Str("result", string(result)).
			Msg("cost ledger: the broker was not told the outcome; the event will be delivered again")
	}
	return result
}

// isPermanentStoreError reports whether the database refused the row for its content, so that
// trying again cannot help: a data exception (class 22) or an integrity violation (class 23).
func isPermanentStoreError(err error) bool {
	var pgErr *pgconn.PgError
	if !errors.As(err, &pgErr) || len(pgErr.Code) < 2 {
		return false
	}
	return pgErr.Code[:2] == "22" || pgErr.Code[:2] == "23"
}

// startDurable binds the durable consumer of the cost stream, creating the stream first if
// this is the first service to run (producers create it too). Replicas of this service share
// the consumer as a queue group.
func (c *Consumer) startDurable(ctx context.Context) (func(), error) {
	if c.conn == nil {
		return nil, errors.New("the signed cost consumer has no NATS connection")
	}
	js, err := c.conn.JetStream()
	if err != nil {
		return nil, fmt.Errorf("initialize JetStream for the cost ledger: %w", err)
	}
	if err := EnsureStream(js); err != nil {
		return nil, err
	}
	subscription, err := js.QueueSubscribe(SubjectCostLedger, DurableName, func(message *nats.Msg) {
		handlerCtx, cancel := context.WithTimeout(ctx, handlerTimeout)
		defer cancel()
		c.handleDelivery(handlerCtx, natsDelivery{message: message})
	},
		nats.Durable(DurableName),
		nats.BindStream(StreamName),
		nats.ManualAck(),
		nats.AckExplicit(),
		nats.AckWait(ackWait),
		nats.MaxAckPending(maxAckPending),
		// Everything the stream holds, the first time the consumer exists: events published
		// before it was created (a producer deployed first) are read, not skipped.
		nats.DeliverAll(),
		// Never give up: an authentic event is retried for as long as the database is down.
		// What cannot succeed (an invalid or refused event) is terminated, not retried.
		nats.MaxDeliver(-1),
	)
	if err != nil {
		return nil, fmt.Errorf("subscribe the durable consumer %s of %s: %w", DurableName, StreamName, err)
	}
	log.Info().Str("stream", StreamName).Str("consumer", DurableName).Str("subject", SubjectCostLedger).
		Msg("usage ledger durable consumer started")
	return func() { _ = subscription.Drain() }, nil
}
