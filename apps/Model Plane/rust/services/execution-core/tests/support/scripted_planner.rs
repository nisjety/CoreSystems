//! A planner that replays a script and records what it was shown.

use std::collections::VecDeque;
use std::sync::Mutex;

use execution_core::browser_agent::planner_view::PlannerView;
use execution_core::browser_agent::BrowserAction;
use execution_core::llm_planner::{
    ActionPlanner, DoneOutcome, DonePayload, PlannerContext, PlannerError, PlannerReply,
    PlannerUsage,
};

/// What the planner saw on one call: the view the loop built (None on the first
/// step) and its rendering, which is exactly the text a model would read.
#[derive(Debug, Clone)]
pub struct Seen {
    pub view: Option<PlannerView>,
    pub rendered: String,
    /// The failure lines the planner was handed (WS11.S3).
    pub failures: Vec<String>,
    /// The retry note, when the call was a retry.
    pub retry_note: Option<String>,
}

#[derive(Default)]
pub struct ScriptedPlanner {
    script: Mutex<VecDeque<Result<PlannerReply, PlannerError>>>,
    seen: Mutex<Vec<Seen>>,
}

impl ScriptedPlanner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn then_action(self, action: BrowserAction) -> Self {
        self.push(Ok(PlannerReply::action(action)))
    }

    pub fn then_action_with_usage(self, action: BrowserAction, input: u32, output: u32) -> Self {
        self.push(Ok(PlannerReply::action(action).with_usage(PlannerUsage {
            input_tokens: input,
            output_tokens: output,
        })))
    }

    pub fn then_done(self) -> Self {
        self.push(Ok(PlannerReply::done()))
    }

    /// A `done` with an answer and an outcome (the honest `done`).
    pub fn then_done_with(self, answer: &str, outcome: DoneOutcome) -> Self {
        self.push(Ok(PlannerReply::done_with(DonePayload::new(answer, outcome))))
    }

    /// Any reply, e.g. one read from raw model text by `reply_from_content`.
    pub fn then_reply(self, reply: PlannerReply) -> Self {
        self.push(Ok(reply))
    }

    pub fn then_error(self, error: PlannerError) -> Self {
        self.push(Err(error))
    }

    fn push(self, reply: Result<PlannerReply, PlannerError>) -> Self {
        self.script
            .lock()
            .expect("script lock")
            .push_back(reply);
        self
    }

    /// Everything the planner was shown, in call order.
    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().expect("seen lock").clone()
    }

    pub fn calls(&self) -> usize {
        self.seen.lock().expect("seen lock").len()
    }
}

#[tonic::async_trait]
impl ActionPlanner for ScriptedPlanner {
    async fn plan_next(&self, ctx: &PlannerContext<'_>) -> Result<PlannerReply, PlannerError> {
        self.seen.lock().expect("seen lock").push(Seen {
            view: ctx.view.cloned(),
            rendered: PlannerView::render_or_first_step(ctx.view),
            failures: ctx.failures.iter().map(|f| f.line()).collect(),
            retry_note: ctx.retry_note.map(str::to_owned),
        });
        // An exhausted script is `done`, so a test never loops forever.
        self.script
            .lock()
            .expect("script lock")
            .pop_front()
            .unwrap_or_else(|| Ok(PlannerReply::done()))
    }
}
