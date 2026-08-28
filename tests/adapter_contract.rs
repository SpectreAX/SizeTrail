use sizetrail::adapters::{
    AdapterDegradedReason, AdapterId, AdapterState, Inventory, ToolchainAdapter,
};
use sizetrail::model::{Advice, Finding};
use sizetrail::policy::PolicyCtx;

struct ContractFixture;

impl ToolchainAdapter for ContractFixture {
    fn id(&self) -> AdapterId {
        AdapterId::new("fixture")
    }

    fn probe(&self, _ctx: &mut PolicyCtx<'_>) -> AdapterState {
        AdapterState::NotPresent
    }

    fn inventory(&self, _ctx: &mut PolicyCtx<'_>, _state: &AdapterState) -> Inventory {
        Inventory
    }

    fn classify(&self, _inventory: &Inventory) -> Vec<Finding> {
        Vec::new()
    }

    fn advise(&self, _finding: &Finding) -> Vec<Advice> {
        Vec::new()
    }
}

#[test]
fn adapter_contract_is_probe_inventory_classify_advise_only() {
    fn requires_contract<T: ToolchainAdapter>() {}

    requires_contract::<ContractFixture>();
}

#[test]
fn absent_tools_and_unknown_versions_are_distinct_states() {
    let absent = AdapterState::NotPresent;
    let unknown = AdapterState::Degraded {
        observed_version: Some("999.0".to_owned()),
        reason: AdapterDegradedReason::UnknownVersion,
    };

    assert_ne!(absent, unknown);
}
