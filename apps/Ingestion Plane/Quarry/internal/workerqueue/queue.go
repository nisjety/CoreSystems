package workerqueue

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/nats-io/nats.go"
	"github.com/nats-io/nats.go/jetstream"

	"github.com/triodelab/quarry/internal/asyncjobs"
)

const (
	defaultAckWait        = 30 * time.Minute
	defaultInProgressTick = 20 * time.Second
)

type Queue struct {
	conn   *nats.Conn
	js     jetstream.JetStream
	client string
}

type Dispatcher interface {
	Dispatch(context.Context, asyncjobs.Message) error
	Cancel(context.Context, asyncjobs.CancelMessage) error
	Close() error
}

type ConsumerGroup struct {
	consumers []jetstream.ConsumeContext
	cancelSub *nats.Subscription
}

func New(sharedURL, token, clientName string) (*Queue, error) {
	if strings.TrimSpace(sharedURL) == "" {
		return nil, nil
	}

	nc, err := nats.Connect(
		sharedURL,
		nats.Token(token),
		nats.Name(clientName),
	)
	if err != nil {
		return nil, fmt.Errorf("connect nats: %w", err)
	}

	js, err := jetstream.New(nc)
	if err != nil {
		nc.Close()
		return nil, fmt.Errorf("jetstream context: %w", err)
	}

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	_, err = js.CreateStream(ctx, jetstream.StreamConfig{
		Name:     asyncjobs.StreamName,
		Subjects: []string{"velion.ingestion.>"},
		MaxAge:   14 * 24 * time.Hour,
		MaxMsgs:  100_000,
	})
	if err != nil && !strings.Contains(strings.ToLower(err.Error()), "stream already exists") {
		nc.Close()
		return nil, fmt.Errorf("ensure stream: %w", err)
	}

	return &Queue{
		conn:   nc,
		js:     js,
		client: clientName,
	}, nil
}

func (q *Queue) Dispatch(ctx context.Context, msg asyncjobs.Message) error {
	if q == nil || q.js == nil {
		return fmt.Errorf("worker queue is not configured")
	}

	data, err := json.Marshal(msg)
	if err != nil {
		return fmt.Errorf("marshal job message: %w", err)
	}

	publishCtx, cancel := withDefaultTimeout(ctx, 5*time.Second)
	defer cancel()

	_, err = q.js.Publish(publishCtx, asyncjobs.ExecuteSubject(msg.Kind), data)
	if err != nil {
		return fmt.Errorf("publish job message: %w", err)
	}
	return nil
}

func (q *Queue) Cancel(ctx context.Context, msg asyncjobs.CancelMessage) error {
	if q == nil || q.conn == nil {
		return nil
	}

	data, err := json.Marshal(msg)
	if err != nil {
		return fmt.Errorf("marshal cancel message: %w", err)
	}

	if deadline, ok := ctx.Deadline(); !ok || time.Until(deadline) <= 0 {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()
	}

	done := make(chan error, 1)
	go func() {
		done <- q.conn.Publish(asyncjobs.CancelSubject, data)
	}()

	select {
	case err := <-done:
		return err
	case <-ctx.Done():
		return ctx.Err()
	}
}

func (q *Queue) StartConsumers(ctx context.Context, durablePrefix string, handler func(context.Context, asyncjobs.Message) error, cancelHandler func(asyncjobs.CancelMessage)) (*ConsumerGroup, error) {
	if q == nil || q.js == nil || q.conn == nil {
		return nil, fmt.Errorf("worker queue is not configured")
	}
	if handler == nil {
		return nil, fmt.Errorf("handler is required")
	}

	group := &ConsumerGroup{}
	for _, kind := range asyncjobs.AllKinds {
		consumer, err := q.js.CreateOrUpdateConsumer(ctx, asyncjobs.StreamName, jetstream.ConsumerConfig{
			Durable:       durableName(durablePrefix, kind),
			AckPolicy:     jetstream.AckExplicitPolicy,
			AckWait:       defaultAckWait,
			MaxDeliver:    5,
			MaxAckPending: 8,
			FilterSubject: asyncjobs.ExecuteSubject(kind),
			BackOff: []time.Duration{
				2 * time.Second,
				10 * time.Second,
				30 * time.Second,
				2 * time.Minute,
				5 * time.Minute,
			},
		})
		if err != nil {
			group.Close()
			return nil, fmt.Errorf("create consumer for %s: %w", kind, err)
		}

		consumeCtx, err := consumer.Consume(func(msg jetstream.Msg) {
			execCtx, cancel := context.WithCancel(ctx)
			defer cancel()

			done := make(chan struct{})
			go heartbeatWhileRunning(msg, done)

			var job asyncjobs.Message
			if err := json.Unmarshal(msg.Data(), &job); err != nil {
				close(done)
				_ = msg.Term()
				return
			}

			if err := handler(execCtx, job); err != nil {
				close(done)
				metadata, metaErr := msg.Metadata()
				if metaErr == nil && metadata != nil && int(metadata.NumDelivered) >= 5 {
					_ = msg.Term()
					return
				}
				_ = msg.NakWithDelay(5 * time.Second)
				return
			}

			close(done)
			_ = msg.Ack()
		})
		if err != nil {
			group.Close()
			return nil, fmt.Errorf("consume %s jobs: %w", kind, err)
		}

		group.consumers = append(group.consumers, consumeCtx)
	}

	if cancelHandler != nil {
		sub, err := q.conn.Subscribe(asyncjobs.CancelSubject, func(msg *nats.Msg) {
			var control asyncjobs.CancelMessage
			if err := json.Unmarshal(msg.Data, &control); err != nil {
				return
			}
			cancelHandler(control)
		})
		if err != nil {
			group.Close()
			return nil, fmt.Errorf("subscribe cancel channel: %w", err)
		}
		group.cancelSub = sub
	}

	go func() {
		<-ctx.Done()
		_ = group.Close()
	}()

	return group, nil
}

func (g *ConsumerGroup) Close() error {
	if g == nil {
		return nil
	}
	var closeErr error
	for _, consumer := range g.consumers {
		if consumer != nil {
			consumer.Stop()
		}
	}
	g.consumers = nil
	if g.cancelSub != nil {
		closeErr = g.cancelSub.Unsubscribe()
		g.cancelSub = nil
	}
	return closeErr
}

func (q *Queue) Close() error {
	if q == nil || q.conn == nil {
		return nil
	}
	q.conn.Close()
	return nil
}

func heartbeatWhileRunning(msg jetstream.Msg, done <-chan struct{}) {
	ticker := time.NewTicker(defaultInProgressTick)
	defer ticker.Stop()

	for {
		select {
		case <-done:
			return
		case <-ticker.C:
			_ = msg.InProgress()
		}
	}
}

func durableName(prefix string, kind asyncjobs.Kind) string {
	base := strings.TrimSpace(prefix)
	if base == "" {
		base = "quarry-worker"
	}
	base = strings.ReplaceAll(base, ".", "-")
	return base + "-" + strings.TrimSpace(string(kind))
}

func withDefaultTimeout(ctx context.Context, timeout time.Duration) (context.Context, context.CancelFunc) {
	if ctx == nil {
		return context.WithTimeout(context.Background(), timeout)
	}
	if deadline, ok := ctx.Deadline(); ok && time.Until(deadline) > 0 {
		return context.WithCancel(ctx)
	}
	return context.WithTimeout(ctx, timeout)
}
