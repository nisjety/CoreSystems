package quarrycontracts

type RunPolicy struct {
	Concurrency Concurrency  `json:"concurrency"`
	Delay       Delay        `json:"delay"`
	Retry       Retry        `json:"retry"`
	Proxy       Proxy        `json:"proxy"`
	Robots      RobotsMode   `json:"robots"`
	Ordering    Ordering     `json:"ordering"`
	Block       BlockPolicy  `json:"block"`
	Checkpoint  Checkpoint   `json:"checkpoint"`
	Determinism Determinism  `json:"determinism"`
}

type Concurrency struct {
	PerRun    uint32 `json:"per_run"`
	PerDomain uint32 `json:"per_domain"`
}

type Delay struct {
	MinMs  uint32 `json:"min_ms"`
	MaxMs  uint32 `json:"max_ms"`
	Jitter bool   `json:"jitter"`
}

type Retry struct {
	Max     uint32      `json:"max"`
	Backoff BackoffKind `json:"backoff"`
	BaseMs  uint32      `json:"base_ms"`
}

type BackoffKind string
const (
	BackoffFixed  BackoffKind = "fixed"
	BackoffLinear BackoffKind = "linear"
	BackoffExp    BackoffKind = "exp"
)

type Proxy struct {
	Strategy  ProxyStrategy `json:"strategy"`
	Pool      *string       `json:"pool,omitempty"`
	StickyKey *string       `json:"sticky_key,omitempty"`
}

type ProxyStrategy string
const (
	ProxyRotate ProxyStrategy = "rotate"
	ProxySticky ProxyStrategy = "sticky"
	ProxyNone   ProxyStrategy = "none"
)

type RobotsMode string
const (
	RobotsStrict  RobotsMode = "strict"
	RobotsRespect RobotsMode = "respect"
	RobotsIgnore  RobotsMode = "ignore"
)

type Ordering string
const (
	OrdFifo     Ordering = "fifo"
	OrdPriority Ordering = "priority"
	OrdLifo     Ordering = "lifo"
)

type BlockPolicy struct {
	On     BlockTrigger `json:"on"`
	Action BlockAction  `json:"action"`
}

type BlockTrigger string
const (
	TrigChallenge   BlockTrigger = "challenge"
	TrigStatus429   BlockTrigger = "status_429"
	TrigStatus403   BlockTrigger = "status_403"
	TrigSuspectBot  BlockTrigger = "suspected_bot"
)

type BlockAction string
const (
	BlkEscalate BlockAction = "escalate"
	BlkRetry    BlockAction = "retry"
	BlkAbort    BlockAction = "abort"
)

type Checkpoint struct {
	EveryNPages *uint32 `json:"every_n_pages,omitempty"`
	EveryS      *uint32 `json:"every_s,omitempty"`
}

type Determinism string
const (
	DetStrict    Determinism = "strict"
	DetBestEff   Determinism = "best_effort"
	DetOff       Determinism = "off"
)

func DefaultRunPolicy() RunPolicy {
	n := uint32(50); s := uint32(60)
	return RunPolicy{
		Concurrency: Concurrency{PerRun: 8, PerDomain: 2},
		Delay:       Delay{MinMs: 500, MaxMs: 2000, Jitter: true},
		Retry:       Retry{Max: 3, Backoff: BackoffExp, BaseMs: 1000},
		Proxy:       Proxy{Strategy: ProxyNone},
		Robots:      RobotsRespect,
		Ordering:    OrdFifo,
		Block:       BlockPolicy{On: TrigChallenge, Action: BlkEscalate},
		Checkpoint:  Checkpoint{EveryNPages: &n, EveryS: &s},
		Determinism: DetBestEff,
	}
}
