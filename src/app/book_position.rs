use super::{Direction, PageTarget, ReadingLocation};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BookPosition(u16);

impl BookPosition {
    pub const fn from_location(location: ReadingLocation) -> Self {
        if location.spine_index + 1 == location.spine_count
            && location.page_index + 1 == location.page_count
        {
            return Self(1000);
        }
        let fraction = location.page_index * 1000 / location.page_count;
        Self(((location.spine_index * 1000 + fraction) / location.spine_count) as u16)
    }

    pub const fn permille(self) -> usize {
        self.0 as usize
    }
    pub const fn percent(self) -> usize {
        self.permille() / 10
    }

    pub(super) fn shift(&mut self, direction: Direction) {
        self.0 = match direction {
            Direction::Left => self.0.saturating_sub(50),
            Direction::Right => self.0.saturating_add(50).min(1000),
            Direction::Up | Direction::Down => self.0,
        };
    }

    pub(super) fn target(self, location: ReadingLocation) -> (usize, PageTarget) {
        if self.0 == 1000 {
            return (location.spine_count - 1, PageTarget::Last);
        }
        let scaled = self.permille() * location.spine_count;
        (
            scaled / 1000,
            PageTarget::Progress {
                page_index: scaled % 1000,
                page_count: 1001,
            },
        )
    }
}
