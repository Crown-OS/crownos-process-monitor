/// A fixed-capacity history that overwrites its oldest sample. It allocates
/// once at construction and never again.
#[derive(Clone, Debug)]
pub struct RingSeries<T> {
    values: Box<[T]>,
    head: usize,
    len: usize,
}

impl<T: Copy + Default> RingSeries<T> {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            values: vec![T::default(); capacity.max(1)].into_boxed_slice(),
            head: 0,
            len: 0,
        }
    }

    pub fn push(&mut self, value: T) {
        self.values[self.head] = value;
        self.head = (self.head + 1) % self.values.len();
        self.len = (self.len + 1).min(self.values.len());
    }

    #[must_use]
    pub fn capacity(&self) -> usize {
        self.values.len()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }

    #[must_use]
    pub fn latest(&self) -> Option<T> {
        (self.len > 0).then(|| self.values[(self.head + self.values.len() - 1) % self.values.len()])
    }

    /// The samples oldest first, as two contiguous slices.
    #[must_use]
    pub fn as_slices(&self) -> (&[T], &[T]) {
        if self.len < self.values.len() {
            (&self.values[..self.len], &[])
        } else {
            let (newer, older) = self.values.split_at(self.head);
            (older, newer)
        }
    }

    #[must_use]
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = T> + '_ {
        let (older, newer) = self.as_slices();
        older.iter().chain(newer).copied()
    }

    /// The most recent `count` samples, oldest first.
    pub fn recent(&self, count: usize) -> impl Iterator<Item = T> + '_ {
        self.iter().skip(self.len.saturating_sub(count))
    }
}

impl RingSeries<f32> {
    /// Marks a period without data (suspend, unavailable source). Charts
    /// break the line here instead of interpolating.
    pub fn push_gap(&mut self) {
        self.push(f32::NAN);
    }

    pub fn max_recent(&self, count: usize) -> Option<f32> {
        self.recent(count)
            .filter(|value| value.is_finite())
            .reduce(f32::max)
    }

    #[must_use]
    pub fn mean_recent(&self, count: usize) -> Option<f32> {
        let (sum, samples) = self
            .recent(count)
            .filter(|value| value.is_finite())
            .fold((0.0_f64, 0_u32), |(sum, samples), value| {
                (sum + f64::from(value), samples + 1)
            });
        (samples > 0).then(|| (sum / f64::from(samples)) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_order_before_and_after_wrap() {
        let mut ring = RingSeries::new(3);
        ring.push(1);
        ring.push(2);
        assert_eq!(ring.iter().collect::<Vec<_>>(), [1, 2]);
        ring.push(3);
        ring.push(4);
        assert_eq!(ring.iter().collect::<Vec<_>>(), [2, 3, 4]);
        assert_eq!(ring.latest(), Some(4));
        assert_eq!(ring.recent(2).collect::<Vec<_>>(), [3, 4]);
    }

    #[test]
    fn statistics_skip_gaps() {
        let mut ring = RingSeries::new(4);
        ring.push(10.0);
        ring.push_gap();
        ring.push(30.0);
        assert_eq!(ring.max_recent(4), Some(30.0));
        assert_eq!(ring.mean_recent(4), Some(20.0));
        assert_eq!(RingSeries::<f32>::new(2).mean_recent(2), None);
    }
}
