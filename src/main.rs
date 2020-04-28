use std::convert::TryInto;
use std::ops::Div;
use std::path::Path;
use std::{thread, time};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

extern crate signal_hook;
extern crate num;

mod fan;
use fan::*;
mod sysfs;
mod polaris_gpu;
use polaris_gpu::*;
mod clamped_percentage;
use clamped_percentage::*;
mod stats;
use stats::*;
mod circular_buffer;
use circular_buffer::CircularBuffer;
mod polaris_gpu_fan;
mod generic_sysfs_fan;
mod nct6797_fan;


#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GpuCustomState {
    Idle,
    CoolOff,
    Performance
}

pub struct GpuStateMachine {
    state: GpuCustomState,
    usage_buffer: CircularBuffer::<f64>,
    temperature_buffer: CircularBuffer::<f32>
}

impl GpuStateMachine {

    pub fn state(&self) -> GpuCustomState {
        self.state
    }

    pub fn new(buffer_scale: usize) -> Self {
        GpuStateMachine {
            state: GpuCustomState::Idle,
            usage_buffer: CircularBuffer::new(30 * buffer_scale),
            temperature_buffer: CircularBuffer::new(10 * buffer_scale)
        }
    }

    pub fn update(&mut self, gpu: &PolarisGpu<'_>) {
        self.usage_buffer.add(gpu.usage().0);
        self.temperature_buffer.add(gpu.temperature());
    }

    pub fn step(&mut self, gpu: &PolarisGpu<'_>){
        let current_temperature = *self.temperature_buffer.last();
        let weighted_avg_usage = index_weighted_average(self.usage_buffer.iter());
        let weighted_avg_temperature = index_weighted_average(self.temperature_buffer.iter());
        let performance_treshold = 75f64;

        println!(" * {}C, weighted usage: {:.2}%, weighted temperature: {:.2}C",
            current_temperature, weighted_avg_usage, weighted_avg_temperature);

        let new_state = if weighted_avg_usage >= performance_treshold {
            GpuCustomState::Performance
        } else {
            match self.state {
                GpuCustomState::Idle => {
                    if weighted_avg_usage > 50f64 {
                        GpuCustomState::Performance
                    } else if current_temperature >= 55f32 {
                        GpuCustomState::CoolOff
                    } else {
                        self.state
                    }
                },
                GpuCustomState::CoolOff => {
                    if weighted_avg_temperature <= 43f32 {
                        GpuCustomState::Idle
                    } else {
                        self.state
                    }
                },
                GpuCustomState::Performance => {
                    if self.usage_buffer.iter().any(|usage| *usage >= performance_treshold) && weighted_avg_usage >= 5f64 {
                        self.state
                    } else {
                        GpuCustomState::Idle
                    }
                }
            }
        };

        if new_state != self.state {
            self.state = new_state;
            self.apply(gpu);
        }
    }

    fn apply(&self, gpu: &PolarisGpu<'_>) {
        println!("> Applying state {:?}", self.state);

        match self.state {
            GpuCustomState::Idle => {
                gpu.set_force_performance_level(PerformanceLevel::Manual);

                gpu.fan().set_mode(FanMode::Manual);
                gpu.fan().set_speed(ClampedPercentage::new(0));
                gpu.set_pcie_level(PcieLevel::Gen1);

                gpu.set_pstate_core(0);
                gpu.set_pstate_memory(0);
                gpu.set_power_limit(40f32);
            },
            GpuCustomState::Performance => {
                gpu.set_force_performance_level(PerformanceLevel::Manual);

                gpu.fan().set_mode(FanMode::Manual);
                gpu.fan().set_speed(ClampedPercentage::new(45));
                gpu.set_pcie_level(PcieLevel::Gen3);

                gpu.set_pstate_core(7);
                gpu.set_pstate_memory(2);
                gpu.set_power_limit(135f32);
            },
            GpuCustomState::CoolOff => {
                gpu.fan().set_mode(FanMode::Manual);
                gpu.fan().set_speed(ClampedPercentage::new(35));
                gpu.set_pcie_level(PcieLevel::Gen1);
            }
        }
    }
}

fn main() {
    let rx570 = PolarisGpu::new("RX 570", Path::new("/sys/class/drm/card0/device/"));
    let term = Arc::new(AtomicBool::new(false));

    signal_hook::flag::register(signal_hook::SIGTERM, Arc::clone(&term)).expect("Failed to register hook for SIGTERM");
    signal_hook::flag::register(signal_hook::SIGINT, Arc::clone(&term)).expect("Failed to register hook for SIGINT");

    let update_interval = time::Duration::from_secs_f32(1f32);
    let gathers_per_update = 2;
    let gathers_per_reapply = 10;

    let sleep_time = update_interval.div(gathers_per_update.try_into().unwrap());

    let mut gathers = 0;

    rx570.modify_pstate_core(7, 1275, 1000).expect("Core overlock failed");
    rx570.modify_pstate_memory(2, 1900, 900).expect("Memory overclock failed");
    rx570.commit_pstates();

    let mut state_machine = GpuStateMachine::new(gathers_per_update);
    state_machine.apply(&rx570);

    while !term.load(Ordering::Relaxed) {

        state_machine.update(&rx570);

        if gathers % gathers_per_update == 0 {

            println!("{} temperature: {}C, fan: {}, state: {:?}", rx570.name,
                rx570.temperature(), rx570.fan().speed(), state_machine.state());

            state_machine.step(&rx570);
        }

        if gathers % gathers_per_reapply == 0 {
            state_machine.apply(&rx570);
        }


        thread::sleep(sleep_time);
        gathers += 1;
    }

    rx570.set_force_performance_level(PerformanceLevel::Auto);
    rx570.reset_pstates();
    println!("Qutting...");
}
