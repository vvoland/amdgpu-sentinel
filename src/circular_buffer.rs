#[derive(Debug)]
pub struct CircularBuffer<T> {
    data: Vec<T>,
    size: usize,
    last: usize
}

impl<T> CircularBuffer<T> {

    pub fn new(size: usize) -> CircularBuffer<T> {
        CircularBuffer { 
            data: Vec::with_capacity(size),
            size: size,
            last: 0
        }
    }

    pub fn add<A: std::convert::Into<T>>(&mut self, value: A) {
        let t_value = value.into();

        if self.data.len() < self.size {
            self.data.push(t_value);
        } else {
            self.data[self.last] = t_value;
            self.last = (self.last + 1) % self.size;
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn iter(&self) -> CircularIterator<'_, T> {
        let len = self.data.len();

        CircularIterator {
            buffer: &self.data,
            cur: self.last,
            rev_cur: if self.last == 0 { len - 1 } else { self.last - 1 },
            left: len
        }
    }

    #[allow(dead_code)]
    pub fn last(&self) -> &T {
        &self.data[self.last]
    }

}

pub struct CircularIterator<'a, T> {
    buffer: &'a Vec::<T>,
    cur: usize,
    rev_cur: usize,
    left: usize
}

impl<'a, T> Iterator for CircularIterator<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.left > 0 {
            let out: &'a T = &self.buffer[self.cur];

            if self.cur == self.buffer.len() - 1 {
                self.cur = 0
            } else {
                self.cur += 1;
            }
            self.left -= 1;

            Some(out)
        } else {
            None
        }
    }
}

impl<'a, T> DoubleEndedIterator for CircularIterator<'a, T> {

    fn next_back(&mut self) -> Option<&'a T> {
        if self.left > 0 {
            let out: &'a T = &self.buffer[self.rev_cur];

            if self.rev_cur == 0 {
                self.rev_cur = self.buffer.len() - 1;
            } else {
                self.rev_cur -= 1;
            }

            self.left -= 1;
            Some(out)
        } else {
            None
        }
    }
}

impl<'a, T> IntoIterator for &'a CircularBuffer<T> {

    type Item = &'a T;
    type IntoIter = CircularIterator<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_it_non_full() {

        let mut buffer = CircularBuffer::<f64>::new(5);

        buffer.add(1);
        buffer.add(2);
        buffer.add(3);

        let mut it = buffer.iter();

        assert_eq!(it.next(), Some(&1f64));
        assert_eq!(it.next(), Some(&2f64));
        assert_eq!(it.next(), Some(&3f64));
    }

    #[test]
    fn forward_it_overflown() {

        let mut buffer = CircularBuffer::<f64>::new(5);

        buffer.add(1);
        buffer.add(2);

        buffer.add(3);
        buffer.add(4);
        buffer.add(5);
        buffer.add(6);
        buffer.add(7);

        let mut it = buffer.iter();

        assert_eq!(it.next(), Some(&3f64));
        assert_eq!(it.next(), Some(&4f64));
        assert_eq!(it.next(), Some(&5f64));
        assert_eq!(it.next(), Some(&6f64));
        assert_eq!(it.next(), Some(&7f64));
    }

    #[test]
    fn reverse_it_overflown() {

        let mut buffer = CircularBuffer::<f64>::new(5);

        buffer.add(1);
        buffer.add(2);

        buffer.add(3);
        buffer.add(4);
        buffer.add(5);
        buffer.add(6);
        buffer.add(7);

        let mut it = buffer.iter().rev();

        assert_eq!(it.next(), Some(&7f64));
        assert_eq!(it.next(), Some(&6f64));
        assert_eq!(it.next(), Some(&5f64));
        assert_eq!(it.next(), Some(&4f64));
        assert_eq!(it.next(), Some(&3f64));
    }
}