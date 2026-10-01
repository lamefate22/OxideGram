//! Pure business models and rules.

pub mod automation;
pub mod identity;
pub mod types;

#[allow(unused_imports)]
pub use automation::{BotButton, ButtonKind, KeyboardRow, MessageMarkup};
#[allow(unused_imports)]
pub use types::{ApiHash, ApiId, ChatId, PhoneNumber, SenderId};
