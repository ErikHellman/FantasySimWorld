//! The language-model client (port of `src/brain/llm`). Nothing here knows about the sim.

pub mod client;
pub mod parse;
pub mod probe;
pub mod queue;
pub mod transport;
pub mod types;

pub use client::{Capabilities, LearnedCapabilities, LlmClient};
pub use queue::{DailyBudget, DailyUsage, DropReason, Job, JobQueue, JobResult, QueueStats};
pub use transport::{FakeTransport, HttpRequest, HttpResponse, LlmTransport, ReqwestTransport};
pub use types::{ChatMessage, LlmError, LlmErrorKind, LlmRequest, LlmResponse, Protocol, Role, ToolCall, ToolDef};
