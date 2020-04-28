use crate::clamped_percentage::ClampedPercentage;

#[derive(PartialEq, Clone, Copy)]
pub enum FanMode {
    Auto,
    Manual
}

pub trait FanControl {
    fn mode(&self) -> FanMode;
    fn set_mode(&self, mode: FanMode);
    fn speed(&self) -> ClampedPercentage;
    fn set_speed(&self, speed: ClampedPercentage);
}
