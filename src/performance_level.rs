#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PerformanceLevel {
    Auto,
    Low,
    High,
    Manual,
    ProfileStandard,
    ProfileMinSclk,
    ProfileMinMclk,
    ProfilePeak
}

pub trait ControllablePerformanceLevel {
    fn performance_level(&self) -> PerformanceLevel;
    fn set_performance_level(&self, level: PerformanceLevel);
}

impl std::fmt::Display for PerformanceLevel {

    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        std::fmt::Debug::fmt(self, f)

    }
}
