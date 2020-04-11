use std::convert::TryFrom;
use std::convert::Into;
use std::fmt;

#[derive(Debug, PartialEq, Clone, Copy)]
pub struct ClampedPercentage(pub f64);

#[derive(Debug)]
pub enum ClampedPercentageError {
    TooLittle,
    TooBig
}

impl fmt::Display for ClampedPercentage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { 
        let diff = self.0 - self.0.floor();

        if diff > 0.009f64 {
            write!(f, "{:.2}%", self.0)
        } else {
            write!(f, "{:.0}%", self.0)
        }
    }
}

impl TryFrom<f64> for ClampedPercentage {
    type Error = ClampedPercentageError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        if value < 0f64 {
            Err(ClampedPercentageError::TooLittle)
        }
        else if value > 100f64 {
            Err(ClampedPercentageError::TooBig)
        }
        else
        {
            Ok(ClampedPercentage(value))
        }
    }
}

impl ClampedPercentage {
    pub fn new<T>(percentage: T) -> Self where T: Into<f64> {
        ClampedPercentage::try_new(percentage).unwrap()
    }

    pub fn try_new<T>(percentage: T) -> Result<Self, ClampedPercentageError> where T: Into<f64> {
        ClampedPercentage::try_from(percentage.into())
    }
}