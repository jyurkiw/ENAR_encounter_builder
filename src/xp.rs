//! Experience points: what a monster is worth by CR, and what a party can
//! face, from the 2024 Dungeon Master's Guide's encounter-building tables.

/// XP for a challenge rating as monster files write it ("1/4", "10").
pub fn for_cr(cr: &str) -> Option<u32> {
    Some(match cr.trim() {
        "0" => 10,
        "1/8" => 25,
        "1/4" => 50,
        "1/2" => 100,
        "1" => 200,
        "2" => 450,
        "3" => 700,
        "4" => 1_100,
        "5" => 1_800,
        "6" => 2_300,
        "7" => 2_900,
        "8" => 3_900,
        "9" => 5_000,
        "10" => 5_900,
        "11" => 7_200,
        "12" => 8_400,
        "13" => 10_000,
        "14" => 11_500,
        "15" => 13_000,
        "16" => 15_000,
        "17" => 18_000,
        "18" => 20_000,
        "19" => 22_000,
        "20" => 25_000,
        "21" => 33_000,
        "22" => 41_000,
        "23" => 50_000,
        "24" => 62_000,
        "25" => 75_000,
        "26" => 90_000,
        "27" => 105_000,
        "28" => 120_000,
        "29" => 135_000,
        "30" => 155_000,
        _ => return None,
    })
}

/// The XP a party can face at each difficulty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub low: u32,
    pub moderate: u32,
    pub high: u32,
}

/// XP budget per character (Low, Moderate, High), by character level.
const PER_CHARACTER: [(u32, u32, u32); 20] = [
    (50, 75, 100),
    (100, 150, 200),
    (150, 225, 400),
    (250, 375, 500),
    (500, 750, 1_100),
    (600, 1_000, 1_400),
    (750, 1_300, 1_700),
    (1_000, 1_700, 2_100),
    (1_300, 2_000, 2_600),
    (1_600, 2_300, 3_100),
    (1_900, 2_900, 4_100),
    (2_200, 3_700, 4_700),
    (2_600, 4_200, 5_400),
    (2_900, 4_900, 6_200),
    (3_300, 5_400, 7_800),
    (3_800, 6_100, 9_800),
    (4_500, 7_200, 11_700),
    (5_000, 8_700, 14_200),
    (5_500, 10_700, 17_200),
    (6_400, 13_200, 22_000),
];

/// The budget for `pcs` characters of `level` (1 to 20).
pub fn budget(level: u32, pcs: u32) -> Option<Budget> {
    let (low, moderate, high) = *PER_CHARACTER.get(level.checked_sub(1)? as usize)?;
    Some(Budget { low: low * pcs, moderate: moderate * pcs, high: high * pcs })
}

/// A difficulty an encounter can aim for. Each covers the XP above the
/// budget below it, up to its own budget: Moderate is more than the Low
/// budget, up to the Moderate one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Tier {
    Low,
    #[default]
    Moderate,
    High,
}

impl Tier {
    pub const ALL: [Tier; 3] = [Tier::Low, Tier::Moderate, Tier::High];

    /// The name the encounter file uses.
    pub fn name(self) -> &'static str {
        match self {
            Tier::Low => "low",
            Tier::Moderate => "moderate",
            Tier::High => "high",
        }
    }

    pub fn parse(name: &str) -> Option<Tier> {
        Tier::ALL.into_iter().find(|t| t.name() == name.trim())
    }
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Tier::Low => "Low",
            Tier::Moderate => "Moderate",
            Tier::High => "High",
        })
    }
}

impl Budget {
    /// A difficulty's budget: the most XP it covers.
    pub fn of(&self, tier: Tier) -> u32 {
        match tier {
            Tier::Low => self.low,
            Tier::Moderate => self.moderate,
            Tier::High => self.high,
        }
    }

    /// The XP range (inclusive) for an encounter meant to be `from` to `to`
    /// difficulty: just over the budget below `from`, up to `to`'s budget.
    pub fn target(&self, from: Tier, to: Tier) -> (u32, u32) {
        let min = match from {
            Tier::Low => 1,
            Tier::Moderate => self.low + 1,
            Tier::High => self.moderate + 1,
        };
        (min, self.of(to.max(from)))
    }
}

/// Where a roster's XP falls against its target range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Below the range: adding `need` to `room` more XP lands in it. Both
    /// ends are given (the spreadsheet's min and max "remaining"), so raising
    /// either end of the target never makes a number shrink.
    Short { need: u32, room: u32 },
    /// In range, with this much XP to spare before its top.
    InRange(u32),
    /// This much XP over the top.
    Over(u32),
}

impl Fit {
    pub fn of(total: u32, (min, max): (u32, u32)) -> Fit {
        if total < min {
            Fit::Short { need: min - total, room: max - total }
        } else if total <= max {
            Fit::InRange(max - total)
        } else {
            Fit::Over(total - max)
        }
    }
}

/// How hard an encounter is: the lowest difficulty whose budget covers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difficulty {
    Empty,
    Low,
    Moderate,
    High,
    OverBudget,
}

impl Difficulty {
    pub fn of(total: u32, budget: Budget) -> Difficulty {
        match total {
            0 => Difficulty::Empty,
            t if t <= budget.low => Difficulty::Low,
            t if t <= budget.moderate => Difficulty::Moderate,
            t if t <= budget.high => Difficulty::High,
            _ => Difficulty::OverBudget,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Empty => "Empty",
            Difficulty::Low => "Low",
            Difficulty::Moderate => "Moderate",
            Difficulty::High => "High",
            Difficulty::OverBudget => "Over the High budget",
        }
    }
}

/// 12345 -> "12,345".
pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xp_by_cr() {
        assert_eq!(for_cr("1/8"), Some(25));
        assert_eq!(for_cr("10"), Some(5_900)); // the aboleth's "XP 5,900"
        assert_eq!(for_cr("21"), Some(33_000)); // the lich's "XP 33,000"
        assert_eq!(for_cr("31"), None);
        assert_eq!(for_cr("?"), None);
    }

    #[test]
    fn budgets_scale_with_party_size() {
        assert_eq!(budget(3, 5), Some(Budget { low: 750, moderate: 1_125, high: 2_000 }));
        assert_eq!(budget(20, 4), Some(Budget { low: 25_600, moderate: 52_800, high: 88_000 }));
        assert_eq!(budget(0, 4), None);
        assert_eq!(budget(21, 4), None);
    }

    #[test]
    fn difficulty_is_the_lowest_budget_that_covers_the_total() {
        let b = budget(3, 5).unwrap();
        assert_eq!(Difficulty::of(0, b), Difficulty::Empty);
        assert_eq!(Difficulty::of(750, b), Difficulty::Low);
        assert_eq!(Difficulty::of(751, b), Difficulty::Moderate);
        assert_eq!(Difficulty::of(1_350, b), Difficulty::High);
        assert_eq!(Difficulty::of(2_001, b), Difficulty::OverBudget);
    }

    #[test]
    fn target_ranges_follow_the_difficulty_bands() {
        let b = budget(3, 5).unwrap(); // 750 / 1,125 / 2,000
        assert_eq!(b.target(Tier::Moderate, Tier::Moderate), (751, 1_125));
        assert_eq!(b.target(Tier::Moderate, Tier::High), (751, 2_000));
        assert_eq!(b.target(Tier::Low, Tier::Low), (1, 750));
        assert_eq!(b.target(Tier::High, Tier::High), (1_126, 2_000));
        assert_eq!(b.target(Tier::High, Tier::Low), (1_126, 2_000)); // an upside-down range
        // The target and the rating agree at the edges.
        assert_eq!(Difficulty::of(751, b), Difficulty::Moderate);
        assert_eq!(Difficulty::of(1_126, b), Difficulty::High);
    }

    #[test]
    fn fit_against_a_target() {
        let range = (751, 1_125);
        assert_eq!(Fit::of(600, range), Fit::Short { need: 151, room: 525 });
        assert_eq!(Fit::of(751, range), Fit::InRange(374));
        assert_eq!(Fit::of(1_125, range), Fit::InRange(0));
        assert_eq!(Fit::of(1_350, range), Fit::Over(225));
    }

    #[test]
    fn raising_the_target_never_shrinks_what_can_be_added() {
        // The reported bug: level 3, 5 PCs, 300 XP. Going from Low-High to
        // Moderate-High showed "1,700 XP to spare" then "451 XP short".
        let b = budget(3, 5).unwrap();
        let room = |from, to| match Fit::of(300, b.target(from, to)) {
            Fit::Short { room, .. } | Fit::InRange(room) => room,
            Fit::Over(_) => 0,
        };
        assert_eq!(room(Tier::Low, Tier::High), 1_700);
        assert_eq!(room(Tier::Moderate, Tier::High), 1_700);
        assert_eq!(Fit::of(300, b.target(Tier::Moderate, Tier::High)), Fit::Short { need: 451, room: 1_700 });
        // Raising the top raises the room; raising the bottom raises the need.
        assert!(room(Tier::Moderate, Tier::High) > room(Tier::Moderate, Tier::Moderate));
        let need = |from, to| match Fit::of(300, b.target(from, to)) {
            Fit::Short { need, .. } => need,
            _ => 0,
        };
        assert!(need(Tier::High, Tier::High) > need(Tier::Moderate, Tier::High));
    }

    #[test]
    fn tier_names_round_trip() {
        for t in Tier::ALL {
            assert_eq!(Tier::parse(t.name()), Some(t));
        }
        assert_eq!(Tier::parse("deadly"), None);
    }

    #[test]
    fn thousands_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(950), "950");
        assert_eq!(thousands(33_000), "33,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(-150), "-150");
    }
}
