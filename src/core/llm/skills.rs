use std::collections::HashMap;
use std::sync::LazyLock;

static CORE_SKILL: &str = include_str!("../../skills/core/skill.md");
static CHAT_SKILL: &str = include_str!("../../skills/chat/skill.md");
static ONCHAIN_SKILL: &str = include_str!("../../skills/onchain/skill.md");
static SETUP_SKILL: &str = include_str!("../../skills/setup/skill.md");

static STRATEGY_CONSERVATIVE: &str =
    include_str!("../../skills/strategies/conservative.md");
static STRATEGY_BALANCED: &str = include_str!("../../skills/strategies/balanced.md");
static STRATEGY_GROWTH: &str = include_str!("../../skills/strategies/growth.md");

static CORE_REF_PERSONALITY: &str =
    include_str!("../../skills/core/references/personality.md");
static CORE_REF_SCOPE: &str = include_str!("../../skills/core/references/scope.md");
static CORE_REF_SAFETY: &str = include_str!("../../skills/core/references/safety.md");
static CORE_REF_SKILL_INDEX: &str =
    include_str!("../../skills/core/references/skill_index.md");
static CORE_REF_TOOL_RESPONSES: &str =
    include_str!("../../skills/core/references/tool_responses.md");

static CHAT_REF_BOUNDARY: &str =
    include_str!("../../skills/chat/references/boundary.md");
static CHAT_REF_POLICY_SCHEMA: &str =
    include_str!("../../skills/chat/references/policy_schema.md");
static CHAT_REF_POLICY_DEFAULTS: &str =
    include_str!("../../skills/chat/references/policy_defaults.md");
static CHAT_REF_TOOLS_INDEX: &str =
    include_str!("../../skills/chat/references/tools_index.md");
static CHAT_REF_WORKFLOW: &str =
    include_str!("../../skills/chat/references/workflow.md");
static CHAT_REF_TOOL_RESPONSES: &str =
    include_str!("../../skills/chat/references/tool_responses.md");

static ONCHAIN_REF_CHECK_BALANCES: &str =
    include_str!("../../skills/onchain/references/check-balances.md");
static ONCHAIN_REF_MOVE_FUNDS: &str =
    include_str!("../../skills/onchain/references/move-funds.md");
static ONCHAIN_REF_SUPPLY_AAVE: &str =
    include_str!("../../skills/onchain/references/supply-aave.md");
static ONCHAIN_REF_WITHDRAW_AAVE: &str =
    include_str!("../../skills/onchain/references/withdraw-aave.md");

static SETUP_REF_ACCOUNT_STATUS: &str =
    include_str!("../../skills/setup/references/account-status.md");
static SETUP_REF_ENSURE_ACCOUNT: &str =
    include_str!("../../skills/setup/references/ensure-account.md");

static ON_DEMAND: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();

    m.insert("core/personality", CORE_REF_PERSONALITY);
    m.insert("core/scope", CORE_REF_SCOPE);
    m.insert("core/safety", CORE_REF_SAFETY);
    m.insert("core/skill_index", CORE_REF_SKILL_INDEX);
    m.insert("core/tool_responses", CORE_REF_TOOL_RESPONSES);

    m.insert("chat/boundary", CHAT_REF_BOUNDARY);
    m.insert("chat/policy_schema", CHAT_REF_POLICY_SCHEMA);
    m.insert("chat/policy_defaults", CHAT_REF_POLICY_DEFAULTS);
    m.insert("chat/tools_index", CHAT_REF_TOOLS_INDEX);
    m.insert("chat/workflow", CHAT_REF_WORKFLOW);
    m.insert("chat/tool_responses", CHAT_REF_TOOL_RESPONSES);

    m.insert("onchain/check-balances", ONCHAIN_REF_CHECK_BALANCES);
    m.insert("onchain/move-funds", ONCHAIN_REF_MOVE_FUNDS);
    m.insert("onchain/supply-aave", ONCHAIN_REF_SUPPLY_AAVE);
    m.insert("onchain/withdraw-aave", ONCHAIN_REF_WITHDRAW_AAVE);
    m.insert("tools/check-balances", ONCHAIN_REF_CHECK_BALANCES);
    m.insert("tools/move-funds", ONCHAIN_REF_MOVE_FUNDS);
    m.insert("tools/supply-aave", ONCHAIN_REF_SUPPLY_AAVE);
    m.insert("tools/withdraw-aave", ONCHAIN_REF_WITHDRAW_AAVE);

    m.insert("setup/account-status", SETUP_REF_ACCOUNT_STATUS);
    m.insert("setup/ensure-account", SETUP_REF_ENSURE_ACCOUNT);

    m
});

#[derive(Clone, Copy, Debug)]
pub enum RiskProfile {
    Conservative,
    Balanced,
    Growth,
}

impl RiskProfile {
    pub fn slice(self) -> &'static str {
        match self {
            Self::Conservative => STRATEGY_CONSERVATIVE,
            Self::Balanced => STRATEGY_BALANCED,
            Self::Growth => STRATEGY_GROWTH,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Conservative => "Conservative",
            Self::Balanced => "Balanced",
            Self::Growth => "Growth",
        }
    }

    pub fn parse(input: &str) -> Self {
        match input.to_ascii_lowercase().as_str() {
            "conservative" | "low" => Self::Conservative,
            "growth" | "aggressive" | "high" => Self::Growth,
            _ => Self::Balanced,
        }
    }
}

pub fn load_on_demand(name: &str) -> Option<&'static str> {
    ON_DEMAND.get(name).copied()
}

pub fn build_system_prompt(risk: RiskProfile, state_summary: Option<&str>) -> String {
    let mut s = String::with_capacity(8 * 1024);
    s.push_str(CORE_SKILL);
    s.push_str("\n\n---\n\n");
    s.push_str(SETUP_SKILL);
    s.push_str("\n\n---\n\n");
    s.push_str(ONCHAIN_SKILL);
    s.push_str("\n\n---\n\n# Active Risk Profile\n\n");
    s.push_str(risk.slice());
    if let Some(state) = state_summary {
        s.push_str("\n\n---\n\n# Live State\n\n");
        s.push_str(state);
    }
    s
}

pub fn build_chat_system_prompt(live_state: Option<&str>) -> String {
    let mut s = String::with_capacity(8 * 1024);
    s.push_str(CORE_SKILL);
    s.push_str("\n\n---\n\n");
    s.push_str(CHAT_SKILL);
    if let Some(state) = live_state {
        s.push_str("\n\n---\n\n# Live State (this user, this turn)\n\n");
        s.push_str(state);
    }
    s
}
