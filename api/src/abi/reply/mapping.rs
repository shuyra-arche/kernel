use crate::model::memory::{MappingInfo, MappingState};

impl crate::abi::reply::FromTriple for MappingInfo {
    fn from_triple(state: usize, fault_cause: usize, fault_address: usize) -> Self {
        let state = match state {
            1 => MappingState::Publishing,
            2 => MappingState::Active,
            3 => MappingState::Retiring,
            4 => MappingState::Settled,
            5 => MappingState::Failed,
            _ => panic!("invalid mapping state"),
        };
        Self {
            state,
            fault_cause,
            fault_address,
        }
    }
}
