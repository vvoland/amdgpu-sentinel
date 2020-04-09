
#[allow(dead_code)]
pub fn average<'a, T: 'a + num::Float, I: IntoIterator<Item=&'a T>>(buffer: I) -> T {

    let f = |acc: (T, usize), (idx, val): (usize, &T)| -> (T, usize) {
        let total_weight = acc.1;
        let sum = acc.0;

        (sum + *val, std::cmp::max(idx + 1, total_weight))
    };

    let (sum, total_weight): (T, usize) = buffer.into_iter()
        .enumerate()
        .fold((T::zero(), 0), f);

    sum / T::from(total_weight).unwrap()
}

pub fn index_weighted_average<'a, 
    T: 'a + num::Float,
    I: DoubleEndedIterator<Item=&'a T>>(it: I) -> T {

    let f = |acc: (T, usize), (idx, val): (usize, &T)| -> (T, usize) {
        let total_weight = acc.1;
        let sum = acc.0;

        let weight = idx + 1;

        let weighted_value = *val * T::from(weight).expect("Non numeric index");

        (sum + weighted_value, total_weight + weight)
    };

    let (sum, total_weight): (T, usize) = it
        .enumerate()
        .fold((T::zero(), 0), f);

    sum / T::from(total_weight).unwrap()
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::circular_buffer::CircularBuffer;

    #[test]
    fn index_weighted_average_full() {
        let mut buffer = CircularBuffer::<f64>::new(5);

        buffer.add(1);
        buffer.add(2);
        buffer.add(3);
        buffer.add(4);
        buffer.add(5);

        let expected: f64 = f64::from(5*5 + 4*4 + 3*3 + 2*2 + 1*1) / f64::from(5+4+3+2+1);
        assert_eq!(index_weighted_average(buffer.iter()), expected);
    }

    #[test]
    fn average_the_same() {
        let mut buffer = CircularBuffer::<f64>::new(5);

        buffer.add(1);
        buffer.add(1);
        buffer.add(1);
        buffer.add(1);
        buffer.add(1);

        assert_eq!(index_weighted_average(buffer.iter()), 1f64);
    }

    #[test]
    fn average_non_full() {
        let mut buffer = CircularBuffer::<f64>::new(5);

        buffer.add(1);
        buffer.add(2);
        buffer.add(3);

        assert_eq!(average(&buffer), 2f64);
    }

    #[test]
    fn average_full() {
        let mut buffer = CircularBuffer::<f64>::new(5);

        buffer.add(1);
        buffer.add(2);
        buffer.add(3);
        buffer.add(4);
        buffer.add(5);

        assert_eq!(average(&buffer), 3f64);
    }

    #[test]
    fn average_overflown() {
        let mut buffer = CircularBuffer::<f64>::new(5);

        // These will be forgotten
        buffer.add(1);
        buffer.add(2);
        buffer.add(3);

        // These are the last
        buffer.add(4);
        buffer.add(5);
        buffer.add(6);
        buffer.add(7);
        buffer.add(8);

        assert_eq!(average(&buffer), 6f64);
    }
}