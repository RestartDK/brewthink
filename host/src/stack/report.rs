use super::{Edges, Transfers};
use serde::{Deserialize, Serialize, Serializer};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct HexAddress(pub u32);
impl Serialize for HexAddress {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&format_args!("0x{:x}", self.0))
    }
}

#[derive(Debug, Serialize)]
pub struct FrontierSite {
    pub caller: String,
    pub address: HexAddress,
    #[serde(flatten)]
    pub transfer: FrontierTransfer,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FrontierTransfer {
    ComputedExternalTarget {
        target: HexAddress,
        symbol: Option<String>,
    },
    ExceptionTransfer {
        instruction: String,
    },
    UnresolvedTransfer {
        instruction: String,
    },
    UnmeasuredTarget {
        target: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Verdict {
    BlockedUnproven,
    BlockedBudget,
    PassLimited,
}
impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BlockedUnproven => "BLOCKED_UNPROVEN",
            Self::BlockedBudget => "BLOCKED_BUDGET",
            Self::PassLimited => "PASS_LIMITED",
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EntryFrames {
    pub task: u64,
    pub library: u64,
    pub effects: u64,
}
impl EntryFrames {
    pub fn required_bytes(&self) -> u64 {
        self.task + self.library.max(self.effects) + super::RESERVE
    }
    pub fn check(&self, available: u64) -> anyhow::Result<u64> {
        let required = self.required_bytes();
        anyhow::ensure!(
            self.task <= 4096 && required <= available,
            "reader stack budget exceeded: required={required}, available={available}"
        );
        Ok(required)
    }
}

#[derive(Debug)]
pub struct Measurements {
    pub entry_frames: EntryFrames,
    pub selected_path_bytes: u64,
    pub selected_path: Vec<String>,
    pub accounted_bytes_with_reserve: u64,
}
#[derive(Debug)]
pub enum Outcome {
    Unproven,
    BudgetExceeded(Measurements),
    Limited(Measurements),
}
impl Outcome {
    pub fn verdict(&self) -> Verdict {
        match self {
            Self::Unproven => Verdict::BlockedUnproven,
            Self::BudgetExceeded(_) => Verdict::BlockedBudget,
            Self::Limited(_) => Verdict::PassLimited,
        }
    }
    pub fn measurements(&self) -> Option<&Measurements> {
        match self {
            Self::Unproven => None,
            Self::BudgetExceeded(value) | Self::Limited(value) => Some(value),
        }
    }
}
impl Serialize for Outcome {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Fields<'a> {
            verdict: Verdict,
            entry_frames: Option<&'a EntryFrames>,
            entry_required_bytes: Option<u64>,
            selected_path_bytes: Option<u64>,
            selected_path: Option<&'a [String]>,
            accounted_bytes_with_reserve: Option<u64>,
        }
        let measured = self.measurements();
        Fields {
            verdict: self.verdict(),
            entry_frames: measured.map(|m| &m.entry_frames),
            entry_required_bytes: measured.map(|m| m.entry_frames.required_bytes()),
            selected_path_bytes: measured.map(|m| m.selected_path_bytes),
            selected_path: measured.map(|m| m.selected_path.as_slice()),
            accounted_bytes_with_reserve: measured.map(|m| m.accounted_bytes_with_reserve),
        }
        .serialize(serializer)
    }
}

#[derive(Debug, Serialize)]
pub struct StackReport {
    #[serde(flatten)]
    pub outcome: Outcome,
    pub whole_program_bound: bool,
    pub available_bytes: u64,
    pub reserve_bytes: u64,
    pub selected_symbol_count: usize,
    pub unsupported_frames: BTreeMap<String, String>,
    pub frames: BTreeMap<String, u64>,
    pub edges: Edges,
    #[serde(serialize_with = "serialize_transfers")]
    pub computed_transfers: BTreeMap<String, Transfers>,
    #[serde(serialize_with = "serialize_transfers")]
    pub discovered_transfer_candidates: BTreeMap<String, Transfers>,
    pub compiler_local_dispatches: BTreeMap<String, Vec<HexAddress>>,
    pub frontier: Vec<FrontierSite>,
    pub selected_not_reached_by_direct_edges: Vec<String>,
    pub unselected_symbol_count: usize,
    pub task_body_symbol: &'static str,
    pub scopes: &'static [&'static str],
    pub limitations: &'static [&'static str],
}
fn serialize_transfers<S: Serializer>(
    data: &BTreeMap<String, Transfers>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let rendered: BTreeMap<_, _> = data
        .iter()
        .filter(|(_, sites)| !sites.is_empty())
        .map(|(name, sites)| {
            (
                name,
                sites
                    .iter()
                    .map(|(address, targets)| {
                        (
                            HexAddress(*address),
                            targets.iter().copied().map(HexAddress).collect::<Vec<_>>(),
                        )
                    })
                    .collect::<BTreeMap<_, _>>(),
            )
        })
        .collect();
    rendered.serialize(serializer)
}
