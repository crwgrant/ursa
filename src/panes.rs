const RATIO_MIN: f32 = 0.15;
const RATIO_MAX: f32 = 0.85;
pub const MAX_PANES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaneSpec {
    Leaf,
    Split {
        axis: SplitAxis,
        ratio: f32,
        first: Box<PaneSpec>,
        second: Box<PaneSpec>,
    },
}

impl Default for PaneSpec {
    fn default() -> Self {
        Self::Leaf
    }
}

impl PaneSpec {
    pub fn leaf_count(&self) -> usize {
        match self {
            Self::Leaf => 1,
            Self::Split { first, second, .. } => first.leaf_count() + second.leaf_count(),
        }
    }

    pub fn render(&self) -> String {
        match self {
            Self::Leaf => "leaf".into(),
            Self::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let axis = match axis {
                    SplitAxis::Horizontal => 'h',
                    SplitAxis::Vertical => 'v',
                };
                format!("{axis}:{ratio}:{first}:{second}", first = first.render(), second = second.render())
            }
        }
    }

    pub fn parse(input: &str) -> Self {
        let mut rest = input.trim();
        parse_spec(&mut rest).filter(|_| rest.is_empty()).unwrap_or(Self::Leaf)
    }
}

pub fn clamp_ratio(ratio: f32) -> f32 {
    if !ratio.is_finite() {
        return 0.5;
    }
    ((ratio * 100.0).round() / 100.0).clamp(RATIO_MIN, RATIO_MAX)
}

pub fn equal_split_ratio(first_leaves: usize, second_leaves: usize) -> f32 {
    let first = first_leaves.max(1) as f32;
    let second = second_leaves.max(1) as f32;
    first / (first + second)
}

pub fn wrap_focus(focused: usize, count: usize, delta: isize) -> usize {
    let count = count.max(1);
    let next = focused as isize + delta;
    let wrapped = next.rem_euclid(count as isize);
    wrapped as usize
}

fn parse_spec(input: &mut &str) -> Option<PaneSpec> {
    if let Some(rest) = input.strip_prefix("leaf") {
        *input = rest;
        return Some(PaneSpec::Leaf);
    }
    let axis = if let Some(rest) = input.strip_prefix('h') {
        *input = rest;
        SplitAxis::Horizontal
    } else if let Some(rest) = input.strip_prefix('v') {
        *input = rest;
        SplitAxis::Vertical
    } else {
        return None;
    };
    *input = input.strip_prefix(':')?;
    let split_at = input.find(':')?;
    let ratio: f32 = input[..split_at].parse().ok()?;
    *input = &input[split_at + 1..];
    let first = parse_spec(input)?;
    *input = input.strip_prefix(':')?;
    let second = parse_spec(input)?;
    Some(PaneSpec::Split {
        axis,
        ratio: clamp_ratio(ratio),
        first: Box::new(first),
        second: Box::new(second),
    })
}

#[cfg(test)]
mod tests {
    use super::{PaneSpec, SplitAxis, clamp_ratio, equal_split_ratio, wrap_focus};

    #[test]
    fn pane_spec_round_trips() {
        let spec = PaneSpec::Split {
            axis: SplitAxis::Horizontal,
            ratio: 0.4,
            first: Box::new(PaneSpec::Leaf),
            second: Box::new(PaneSpec::Split {
                axis: SplitAxis::Vertical,
                ratio: 0.55,
                first: Box::new(PaneSpec::Leaf),
                second: Box::new(PaneSpec::Leaf),
            }),
        };
        assert_eq!(spec.leaf_count(), 3);
        assert_eq!(PaneSpec::parse(&spec.render()), spec);
        assert_eq!(PaneSpec::parse("leaf"), PaneSpec::Leaf);
        assert_eq!(PaneSpec::parse("nope"), PaneSpec::Leaf);
    }

    #[test]
    fn ratio_and_focus_wrap() {
        assert_eq!(clamp_ratio(0.501), 0.5);
        assert_eq!(clamp_ratio(0.01), 0.15);
        assert_eq!(clamp_ratio(2.0), 0.85);
        assert_eq!(wrap_focus(0, 3, -1), 2);
        assert_eq!(wrap_focus(2, 3, 1), 0);
        assert_eq!(wrap_focus(1, 3, 1), 2);
        assert_eq!(equal_split_ratio(1, 1), 0.5);
        assert!((equal_split_ratio(1, 2) - 1.0 / 3.0).abs() < f32::EPSILON);
        assert_eq!(equal_split_ratio(1, 3), 0.25);
        assert_eq!(equal_split_ratio(2, 2), 0.5);
    }
}
