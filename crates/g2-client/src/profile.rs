//! The player's profile: per-general experience, rank, bought and slotted perks, after the
//! alpha's Perks 3.0 (Patch Preview #2, October 2013, archive/wayback): every general starts at
//! rank 1 with one perk slot and gains a slot per rank up to 5; experience is earned on the
//! general played; promoting and buying a perk spend it; a perk needs its rank and, for a
//! higher tier, the lower one bought; a bought perk is kept for good. The alpha also charged
//! CP to slot a perk; we have no second currency, so slotting is free.
//!
//! Saved as plain text at `$G2_PROFILE`, else `$XDG_DATA_HOME/generals2/profile`, else
//! `~/.local/share/generals2/profile`. One line per general:
//! `general <name> xp <n> rank <n> bought <id,..> slotted <id,..>`, perks by id.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bevy::prelude::*;
use g2_sim::defs::{GeneralId, PerkId, Rules};

/// The highest general rank: one perk slot per rank.
pub const MAX_RANK: u8 = g2_sim::defs::PERK_SLOTS as u8;

/// Experience to promote from `rank` to the next one: the alpha's Promote button showed
/// 3000 XP at rank 3, so 1000 per rank reached (a placeholder curve).
pub fn promotion_cost(rank: u8) -> u32 {
    1000 * rank as u32
}

/// Experience a finished match earns the general played: 100 for playing, 400 more for a win,
/// and 1 per enemy unit or building destroyed (placeholders; the alpha's amounts aren't
/// archived).
pub fn match_experience(won: bool, kills: u32) -> u32 {
    100 + if won { 400 } else { 0 } + kills
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeneralProgress {
    pub xp: u32,
    pub rank: u8,
    pub bought: Vec<PerkId>,
    pub slotted: Vec<PerkId>,
}

impl GeneralProgress {
    fn fresh() -> GeneralProgress {
        GeneralProgress { rank: 1, ..Default::default() }
    }
}

/// Why a profile change was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refused {
    MaxRank,
    NotEnoughXp,
    RankTooLow,
    NeedsLowerTier,
    NotThisGenerals,
    NotBought,
    NoFreeSlot,
    AlreadySlotted,
}

#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    pub generals: BTreeMap<GeneralId, GeneralProgress>,
    /// Where it is saved; `None` keeps it in memory only (tests).
    pub path: Option<PathBuf>,
}

impl Profile {
    /// The default save location.
    pub fn default_path() -> Option<PathBuf> {
        if let Some(p) = std::env::var_os("G2_PROFILE") {
            return Some(PathBuf::from(p));
        }
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
        Some(data.join("generals2/profile"))
    }

    /// Reads the profile at `path`; a missing file is an empty profile. Lines that don't parse,
    /// or name a general or perk `rules` don't have, are dropped, so an old file never breaks
    /// the game.
    pub fn load(path: Option<PathBuf>, rules: &Rules) -> Profile {
        let text = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        let mut generals = BTreeMap::new();
        for line in text.lines() {
            if let Some((g, p)) = parse_line(line, rules) {
                generals.insert(g, p);
            }
        }
        Profile { generals, path }
    }

    /// Writes the profile; errors are reported and ignored (progress is not worth a crash).
    pub fn save(&self, rules: &Rules) {
        let Some(path) = &self.path else { return };
        let ids = |v: &[PerkId]| v.iter().map(|k| k.0.to_string()).collect::<Vec<_>>().join(",");
        let mut out = String::new();
        for (g, p) in &self.generals {
            out += &format!(
                "general {} xp {} rank {} bought {} slotted {}\n",
                rules.general(*g).name,
                p.xp,
                p.rank,
                ids(&p.bought),
                ids(&p.slotted)
            );
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(path, out) {
            eprintln!("profile: can't save {}: {e}", path.display());
        }
    }

    pub fn progress(&self, g: GeneralId) -> GeneralProgress {
        self.generals.get(&g).cloned().unwrap_or_else(GeneralProgress::fresh)
    }

    fn entry(&mut self, g: GeneralId) -> &mut GeneralProgress {
        self.generals.entry(g).or_insert_with(GeneralProgress::fresh)
    }

    /// The perks slotted on `g`, for a match's `PlayerSetup::perks`.
    pub fn slotted(&self, g: Option<GeneralId>) -> Vec<PerkId> {
        g.map_or_else(Vec::new, |g| self.progress(g).slotted)
    }

    pub fn earn(&mut self, g: GeneralId, xp: u32) {
        self.entry(g).xp += xp;
    }

    pub fn promote(&mut self, g: GeneralId) -> Result<(), Refused> {
        let p = self.entry(g);
        if p.rank >= MAX_RANK {
            return Err(Refused::MaxRank);
        }
        let cost = promotion_cost(p.rank);
        if p.xp < cost {
            return Err(Refused::NotEnoughXp);
        }
        p.xp -= cost;
        p.rank += 1;
        Ok(())
    }

    pub fn buy(&mut self, rules: &Rules, g: GeneralId, perk: PerkId) -> Result<(), Refused> {
        if !rules.general(g).perks.contains(&perk) {
            return Err(Refused::NotThisGenerals);
        }
        let k = rules.perk(perk);
        let p = self.entry(g);
        if p.bought.contains(&perk) {
            return Ok(());
        }
        if p.rank < k.rank {
            return Err(Refused::RankTooLow);
        }
        if k.requires.is_some_and(|r| !p.bought.contains(&r)) {
            return Err(Refused::NeedsLowerTier);
        }
        if p.xp < k.price {
            return Err(Refused::NotEnoughXp);
        }
        p.xp -= k.price;
        p.bought.push(perk);
        p.bought.sort();
        Ok(())
    }

    /// Slots a bought perk into a free slot (one per rank).
    pub fn slot(&mut self, g: GeneralId, perk: PerkId) -> Result<(), Refused> {
        let p = self.entry(g);
        if !p.bought.contains(&perk) {
            return Err(Refused::NotBought);
        }
        if p.slotted.contains(&perk) {
            return Err(Refused::AlreadySlotted);
        }
        if p.slotted.len() >= p.rank as usize {
            return Err(Refused::NoFreeSlot);
        }
        p.slotted.push(perk);
        Ok(())
    }

    pub fn unslot(&mut self, g: GeneralId, perk: PerkId) {
        self.entry(g).slotted.retain(|&k| k != perk);
    }
}

fn parse_line(line: &str, rules: &Rules) -> Option<(GeneralId, GeneralProgress)> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let field = |key: &str| words.iter().position(|&w| w == key).and_then(|i| words.get(i + 1).copied());
    let g = rules.find_general(field("general")?)?;
    let perks = |key: &str| -> Vec<PerkId> {
        field(key)
            .unwrap_or("")
            .split(',')
            .filter_map(|s| s.parse::<u16>().ok().map(PerkId))
            .filter(|k| rules.general(g).perks.contains(k))
            .collect()
    };
    let rank = field("rank")?.parse::<u8>().ok()?.clamp(1, MAX_RANK);
    let bought = perks("bought");
    let mut slotted: Vec<PerkId> = perks("slotted").into_iter().filter(|k| bought.contains(k)).collect();
    slotted.dedup();
    slotted.truncate(rank as usize);
    Some((g, GeneralProgress { xp: field("xp")?.parse().ok()?, rank, bought, slotted }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use g2_rules::generals::{BULLDOG, JUNKYARD};
    use g2_rules::perks::*;

    fn rules() -> &'static Rules {
        g2_rules::rules()
    }

    /// A fresh general has rank 1 and one slot; buying needs the rank and, for a second tier,
    /// the first; promoting costs experience and adds a slot.
    #[test]
    fn ranks_gate_perks_and_slots() {
        let mut p = Profile::default();
        p.earn(BULLDOG, 5_000);
        assert_eq!(p.buy(rules(), BULLDOG, BD_PROTECTOR_II), Err(Refused::RankTooLow));
        p.buy(rules(), BULLDOG, BD_PROTECTOR_I).unwrap();
        p.buy(rules(), BULLDOG, BD_DOUBLE_SHIFTS).unwrap();
        p.slot(BULLDOG, BD_PROTECTOR_I).unwrap();
        assert_eq!(p.slot(BULLDOG, BD_DOUBLE_SHIFTS), Err(Refused::NoFreeSlot), "rank 1 has one slot");
        p.promote(BULLDOG).unwrap();
        p.promote(BULLDOG).unwrap();
        assert_eq!(p.progress(BULLDOG).rank, 3);
        p.slot(BULLDOG, BD_DOUBLE_SHIFTS).unwrap();
        assert_eq!(p.buy(rules(), JUNKYARD, BD_PROTECTOR_I), Err(Refused::NotThisGenerals));
        p.buy(rules(), BULLDOG, BD_PROTECTOR_II).unwrap();
        let spent = rules().perk(BD_PROTECTOR_I).price
            + rules().perk(BD_DOUBLE_SHIFTS).price
            + promotion_cost(1)
            + promotion_cost(2)
            + rules().perk(BD_PROTECTOR_II).price;
        assert_eq!(p.progress(BULLDOG).xp, 5_000 - spent);
        assert_eq!(p.slotted(Some(BULLDOG)), vec![BD_PROTECTOR_I, BD_DOUBLE_SHIFTS]);
    }

    /// Experience is earned and spent per general, and a refused purchase spends nothing.
    #[test]
    fn experience_belongs_to_one_general() {
        let mut p = Profile::default();
        p.earn(JUNKYARD, match_experience(true, 12));
        assert_eq!(p.progress(JUNKYARD).xp, 512);
        assert_eq!(p.progress(BULLDOG).xp, 0);
        assert_eq!(p.buy(rules(), BULLDOG, BD_DOUBLE_SHIFTS), Err(Refused::NotEnoughXp));
        assert_eq!(p.promote(JUNKYARD), Err(Refused::NotEnoughXp));
        assert_eq!(p.progress(JUNKYARD).xp, 512);
    }

    /// A saved profile reads back the same; a line naming an unknown general, or a perk that
    /// isn't the general's, is dropped rather than breaking the game.
    #[test]
    fn profiles_round_trip_through_the_file() {
        let dir = std::env::temp_dir().join(format!("g2-profile-{}", std::process::id()));
        let path = dir.join("profile");
        let mut p = Profile { path: Some(path.clone()), ..Default::default() };
        p.earn(BULLDOG, 4_000);
        p.buy(rules(), BULLDOG, BD_PROTECTOR_I).unwrap();
        p.promote(BULLDOG).unwrap();
        p.slot(BULLDOG, BD_PROTECTOR_I).unwrap();
        p.save(rules());
        assert_eq!(Profile::load(Some(path.clone()), rules()), p);
        let junk = format!(
            "general Nobody xp 5 rank 2 bought 1 slotted 1\ngeneral Junkyard xp 7 rank 9 bought {},{} slotted {},{}\n",
            JY_SCAVENGER.0, BD_PROTECTOR_I.0, JY_SCAVENGER.0, BD_PROTECTOR_I.0
        );
        std::fs::write(&path, junk).unwrap();
        let q = Profile::load(Some(path), rules());
        let j = q.progress(JUNKYARD);
        assert_eq!((q.generals.len(), j.xp, j.rank), (1, 7, MAX_RANK));
        assert_eq!((j.bought, j.slotted), (vec![JY_SCAVENGER], vec![JY_SCAVENGER]));
        let _ = std::fs::remove_dir_all(dir);
    }
}
