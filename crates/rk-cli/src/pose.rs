// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! `rk animation` and `rk clothes`: what CharaStudio's animation list holds and
//! how it lands on a character's bones, and what the states of a card's
//! clothes show, as text to check against the game's code.

use anyhow::{Context, Result};
use rk_card::Card;
use rk_chara::animations::Library;
use rk_chara::clothes_state::{self as cs, States, KINDS};
use rk_chara::motion::{self, Parameters, Skeleton};
use rk_chara::BuildOptions;
use rk_vfs::Game;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Lists the animations, by category.
pub fn list(details: bool) -> Result<()> {
    let game = Game::open_user(&[])?;
    let mut library = Library::read(&game);
    if details {
        library.time(&game, &|| false);
    }
    let parameters = Parameters {
        height: 0.5,
        breast: 0.5,
        options: None,
    };
    for category in library.categories() {
        println!(
            "{} ({} of group {} category {})",
            category.name, category.count, category.group, category.category
        );
        if !details {
            continue;
        }
        for pose in library
            .entries
            .iter()
            .filter(|p| (p.group, p.category) == (category.group, category.category))
        {
            let listed = match pose.timing {
                Some(t) => format!(
                    "{} {:.2} s{}",
                    if t.is_static() { "static" } else { "dynamic" },
                    t.seconds,
                    if t.loops { ", loops" } else { "" }
                ),
                None => "not read".to_owned(),
            };
            let kind = match motion::load(
                &game,
                &pose.bundle,
                &pose.controller,
                &pose.state,
                pose.over(),
                &parameters,
            ) {
                Ok(m) => format!("{listed}, moves {:.4}", m.movement()),
                Err(e) => format!("unreadable: {e:#}"),
            };
            println!(
                "  {:>3} {:<34} {:<28} {}/{}  {kind}",
                pose.number, pose.name, pose.original, pose.controller, pose.state
            );
        }
    }
    println!("{} animations", library.entries.len());
    Ok(())
}

/// Writes the animations of a category, or one of them, as Renkatsu
/// animation files under `<out>/<Group>/<Category>/`, the layout of a
/// package's `animations` folder.
pub fn export(group: i32, category: i32, number: Option<i32>, out: &Path) -> Result<()> {
    let game = Game::open_user(&[])?;
    let library = Library::read(&game);
    let wanted: Vec<_> = library
        .entries
        .iter()
        .filter(|e| (e.group, e.category) == (group, category) && number.is_none_or(|n| n == e.number))
        .collect();
    if wanted.is_empty() {
        anyhow::bail!("the list has no animation with these numbers; `rk animation list` prints them");
    }
    let (group_name, category_name) = (library.group_name(group), library.category_label(group, category));
    let clean = |name: &str| -> String {
        let s: String = name
            .chars()
            .map(|c| {
                if "/\\:*?\"<>|".contains(c) || c.is_control() {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        s.trim().trim_end_matches('.').to_owned()
    };
    let dir = out.join(clean(&group_name)).join(clean(&category_name));
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let mut total = 0;
    for entry in wanted {
        let mut stored = rk_chara::animation_file::Stored::extract(
            &game,
            &entry.bundle,
            &entry.controller,
            &entry.state,
            entry.over(),
        )?;
        stored.meta.name = entry.name.clone();
        stored.meta.group = group_name.clone();
        stored.meta.category = category_name.clone();
        stored.meta.source = entry.source.label().to_owned();
        stored.meta.h = entry.h;
        let file = dir.join(format!(
            "{}.{}",
            clean(&entry.name),
            rk_chara::animation_file::EXTENSION
        ));
        let bytes = stored.to_bytes();
        total += bytes.len();
        std::fs::write(&file, &bytes).with_context(|| format!("writing {}", file.display()))?;
        println!("{}  {} bytes", file.display(), bytes.len());
    }
    println!("{total} bytes in all");
    Ok(())
}

/// Reads every animation and binds it to the skeleton of a card's
/// character, and says what did not fit.
/// Prints CharaStudio's FK bones as found on a card's character: by part,
/// each bone's number, object, shown name and level, then those of the
/// table the character does not have.
pub fn bones(card: &Path) -> Result<()> {
    use rk_chara::bone_info::{self, Part};
    let game = Game::open_user(&[])?;
    let card = Card::load(card)?;
    let options = BuildOptions {
        clothes: false,
        accessories: false,
        physics: false,
        ..Default::default()
    };
    let chara = rk_chara::build(&game, &card, &options)?;
    let table = bone_info::read(&game);
    let female = card.sex() == 1;
    let found = bone_info::on_character(&table, &chara.scene, chara.animator, chara.top, female);
    println!(
        "{} bones in the table, {} on {} ({})",
        table.len(),
        found.len(),
        card.full_name(),
        if female { "female" } else { "male" }
    );
    for part in Part::ALL {
        let bones: Vec<_> = found.iter().filter(|b| b.info.parts().contains(&part)).collect();
        println!("\n{} ({}):", part.label(), bones.len());
        for b in bones {
            println!(
                "  {:>4}  {:<28} {}  level {}",
                b.info.no, b.info.bone, b.info.name, b.info.level
            );
        }
    }
    let missing: Vec<_> = table
        .values()
        .filter(|i| !(female && i.level == 2))
        .filter(|i| !found.iter().any(|b| b.info.no == i.no))
        .collect();
    if !missing.is_empty() {
        println!("\nNot on this character ({}):", missing.len());
        for i in missing {
            println!("  {:>4}  {:<28} {}  group {}", i.no, i.bone, i.name, i.group);
        }
    }
    Ok(())
}

pub fn check(card: &Path) -> Result<()> {
    let game = Game::open_user(&[])?;
    let card = Card::load(card)?;
    let options = BuildOptions {
        clothes: false,
        accessories: false,
        physics: false,
        ..Default::default()
    };
    let chara = rk_chara::build(&game, &card, &options)?;
    let skeleton = Skeleton::of(&chara.scene, chara.animator);
    let library = Library::read(&game);
    println!(
        "{} animations; the Animator of {} has {} transforms below it; height {:.3}, bust {:.3}",
        library.entries.len(),
        card.full_name(),
        skeleton.len(),
        chara.parameters.height,
        chara.parameters.breast
    );
    let (mut failed, mut still, mut moving, mut looping, mut blended, mut transitions) = (0, 0, 0, 0, 0, 0);
    let (mut tracks, mut unbound, mut other) = (0usize, 0usize, 0usize);
    let mut written: BTreeSet<usize> = BTreeSet::new();
    let mut unbound_in: BTreeMap<String, usize> = BTreeMap::new();
    for pose in &library.entries {
        let motion = match motion::load(
            &game,
            &pose.bundle,
            &pose.controller,
            &pose.state,
            pose.over(),
            &chara.parameters,
        ) {
            Ok(m) => m,
            Err(e) => {
                failed += 1;
                println!(
                    "  unreadable: {} ({}/{}): {e:#}",
                    pose.name, pose.controller, pose.state
                );
                continue;
            }
        };
        if motion.clips.is_empty() {
            failed += 1;
            println!("  no clip: {} ({}/{})", pose.name, pose.controller, pose.state);
            continue;
        }
        if motion.movement() <= rk_chara::animations::STILL {
            still += 1;
        } else {
            moving += 1;
        }
        looping += usize::from(motion.loops());
        blended += usize::from(motion.clips.len() > 1);
        transitions += usize::from(motion.transitions > 0);
        other += motion
            .clips
            .iter()
            .flat_map(|(c, _)| &c.bindings)
            .filter(|b| b.target == rk_unityfs::animation::Target::Other)
            .count();
        let bound = motion.bind(&skeleton);
        tracks += bound.tracks();
        unbound += bound.unbound;
        if bound.unbound > 0 {
            *unbound_in.entry(pose.bundle.clone()).or_default() += 1;
        }
        written.extend(bound.nodes());
    }
    println!(
        "{still} hold one pose, {moving} move ({looping} loop); {blended} blend clips by a parameter; \
         {transitions} are states with transitions; {failed} could not be read"
    );
    println!(
        "{tracks} bone properties written in all; {unbound} curves name a transform the skeleton lacks; \
         {other} curves are not a transform's"
    );
    for (bundle, count) in &unbound_in {
        println!("  {count} animations of {bundle} name transforms the skeleton lacks");
    }
    // Which families of bones the animations write.
    let mut families: BTreeMap<String, usize> = BTreeMap::new();
    for &node in &written {
        let name = &chara.scene.nodes[node].name;
        let family: String = name.split('_').take(2).collect::<Vec<_>>().join("_");
        *families.entry(family).or_default() += 1;
    }
    let families: Vec<String> = families.iter().map(|(f, n)| format!("{f} {n}")).collect();
    println!("{} bones written: {}", written.len(), families.join(", "));
    Ok(())
}

fn text(states: &States) -> String {
    states.iter().map(u8::to_string).collect::<Vec<_>>().join("")
}

/// Parses states given as nine digits, or as `kind=state` pairs applied
/// in order with the game's rules.
pub fn parse_states(spec: &str, worn: &cs::Worn) -> Result<States> {
    let mut states = [cs::ON; KINDS];
    if spec.len() == KINDS && spec.bytes().all(|b| (b'0'..=b'3').contains(&b)) {
        for (state, digit) in states.iter_mut().zip(spec.bytes()) {
            *state = digit - b'0';
        }
        return Ok(states);
    }
    for pair in spec.split(',').filter(|p| !p.is_empty()) {
        let (kind, state) = pair
            .split_once('=')
            .with_context(|| format!("{pair}: expected kind=state"))?;
        let state: u8 = state.parse().with_context(|| format!("{pair}: the state is 0 to 3"))?;
        if kind == "all" {
            cs::set_all(&mut states, worn, state);
            continue;
        }
        let kind = NAMES
            .iter()
            .position(|n| *n == kind)
            .with_context(|| format!("{kind}: the kinds are {}", NAMES.join(", ")))?;
        cs::set_state(&mut states, worn, kind, state, true);
    }
    Ok(states)
}

const NAMES: [&str; KINDS] = [
    "top",
    "bottom",
    "bra",
    "shorts",
    "gloves",
    "pantyhose",
    "socks",
    "indoor",
    "outdoor",
];

/// Prints what a card's clothes say about their states, and what each of
/// a row of state changes shows.
pub fn clothes(card: &Path, outfit: usize, steps: &[String]) -> Result<()> {
    let game = Game::open_user(&[])?;
    let card = Card::load(card)?;
    let options = BuildOptions {
        outfit,
        accessories: false,
        physics: false,
        ..Default::default()
    };
    let mut chara = rk_chara::build(&game, &card, &options)?;
    let worn = chara.clothes.worn.clone();
    let kinds = worn.kinds();
    println!("{}, outfit {outfit}", card.full_name());
    for (kind, name) in NAMES.iter().enumerate() {
        let states: Vec<String> = kinds.states(kind).iter().map(u8::to_string).collect();
        println!(
            "  {name:<10} StateType {:<5} states {}",
            worn.state_type[kind].map_or("none".to_owned(), |t| t.to_string()),
            if states.is_empty() {
                "none".to_owned()
            } else {
                states.join(" ")
            }
        );
    }
    println!(
        "  notBot {} notBra {} notShorts {} hideShortsBot {}; parts: top has bottom {}, bottom has top {}, \
         bra has bottom {}, shorts have top {}",
        worn.not_bot,
        worn.not_bra,
        worn.not_shorts,
        worn.hide_shorts_bot,
        worn.top_has_bottom,
        worn.bottom_has_top,
        worn.bra_has_bottom,
        worn.shorts_have_top
    );
    let show = |chara: &rk_chara::Character, states: &States, what: &str| {
        let objects = chara.clothes.objects(&chara.scene);
        let names = |want: bool| -> String {
            objects
                .iter()
                .filter(|(_, on)| *on == want)
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        };
        let shown = cs::shown(states, &worn, chara.clothes.outdoor_shoes, true);
        println!("{what} -> {} alpha {:?}", text(states), shown.alpha);
        println!("    on:  {}", names(true));
        println!("    off: {}", names(false));
    };
    let mut states = [cs::ON; KINDS];
    show(&chara, &states, "as built");
    for step in steps {
        let (kind, state) = step
            .split_once('=')
            .with_context(|| format!("{step}: expected kind=state"))?;
        let state: u8 = state.parse().with_context(|| format!("{step}: the state is 0 to 3"))?;
        if kind == "all" {
            cs::set_all(&mut states, &worn, state);
        } else {
            let kind = NAMES
                .iter()
                .position(|n| *n == kind)
                .with_context(|| format!("{kind}: the kinds are {}", NAMES.join(", ")))?;
            cs::set_state(&mut states, &worn, kind, state, true);
        }
        chara.set_clothes(&states);
        show(&chara, &states, step);
    }
    Ok(())
}

/// Lists the mods that bring animation lists, with how big each is, how
/// many animations it lists and where their clips are.
pub fn mods() -> Result<()> {
    let game = Game::open_user(&[])?;
    let index = &game.assets.index;
    let mut by_mod: BTreeMap<u32, (usize, BTreeSet<String>)> = BTreeMap::new();
    for sheet in &index.studio_sheets {
        let Some(mod_idx) = sheet.mod_idx else { continue };
        // Both shapes of animation sheet count, CharaStudio's own and the
        // H scenes'; a mod whose only list files are items or maps brings
        // no animations and belongs in none of this.
        let Some(last) = rk_chara::animations::animation_sheet(&sheet.name) else {
            continue;
        };
        let entry = by_mod.entry(mod_idx).or_default();
        for row in sheet
            .rows
            .iter()
            .skip(2)
            .filter(|r| r.len() > last && (0..3).all(|i| r[i].trim().parse::<i32>().is_ok()))
        {
            entry.0 += 1;
            entry.1.insert(row[4].to_lowercase());
        }
    }
    let mut total = 0;
    for (mod_idx, (entries, bundles)) in &by_mod {
        let info = &index.mods[*mod_idx as usize];
        let file = if Path::new(&info.path).is_absolute() {
            Path::new(&info.path).to_owned()
        } else {
            index.game_dir.join(&info.path)
        };
        let size = file.metadata().map(|m| m.len()).unwrap_or(0);
        total += size;
        // Whether the mod carries the clips itself or plays the game's.
        let own = bundles
            .iter()
            .filter(|b| {
                index
                    .bundles
                    .get(&rk_vfs::index::norm(b))
                    .is_some_and(|locs| locs.iter().any(|l| l.mod_idx == Some(*mod_idx)))
            })
            .count();
        println!(
            "{:>9}  {entries:>4} animations  {own}/{} bundles its own  {}  ({})",
            rk_packages::size_text(size),
            bundles.len(),
            info.guid,
            info.path
        );
    }
    println!(
        "{} mods bring animation lists, {} in all",
        by_mod.len(),
        rk_packages::size_text(total)
    );
    Ok(())
}
