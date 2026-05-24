use alloy::primitives::Address;

use crate::core::llm::skills::RiskProfile;
use crate::state::AppState;

#[derive(Clone)]
pub struct ToolContext {
    pub state: AppState,
    pub user: Address,
    pub risk: RiskProfile,
}

impl ToolContext {
    pub fn new(state: AppState, user: Address, risk: RiskProfile) -> Self {
        Self { state, user, risk }
    }
}
