# World and biome scale: games vs. Earth

Research notes for configurable world/biome scale. Collected 2026-11-24 by web search.

Confidence marks:
- **[V]** verified from the cited source (official doc, wiki page read directly, or paper text read directly)
- **[A]** approximate / secondary report (news article, guide site, mod docs quoting game internals)
- **[C]** community estimate (forum or Reddit measurement, fan calculation)
- **[U]** could not verify; given from general knowledge or derived, treat as a placeholder

Fandom wikis (valheim.fandom.com) returned HTTP 402 to automated fetching, so Valheim numbers come from secondary sources that quote the game's generator.

---

## 1. Summary table

| Game | World size | Biome / region size | Highest relief | Player speed | View distance | Units |
|---|---|---|---|---|---|---|
| Minecraft (Java, 1.18+) | ±29,999,984 blocks border (~60,000 km across) [V] | Not officially published; 1.18 roughly doubled average biome size [A]; "Large Biomes" ≈ 4× area [V]. Typical biome a few hundred to ~1,000+ blocks across [U] | Build range Y −64..319 (384 blocks) [V]; terrain caps near Y 256, i.e. ~190 blocks above sea level (Y 63) [A] | Walk 4.317 m/s, sprint 5.612 m/s [V] | Render 2–32 chunks (32–512 blocks) [V] | 1 block = 1 m [V by convention of the wiki's m/s figures] |
| Valheim | Disc, land to r = 10,000 m, sea floor drops 10,000–10,500 m (~314 km² disc) [A] | Biomes placed by distance bands (thousands of m) + noise; individual patches roughly 0.5–2 km across [C/U] | Mountains = normalised height > 0.4; water level 30 m [A]; max peak in metres not found [U] | Walk 4, run 7–8.75 (skill), sneak/swim 2 m/s [A]; Longship 5.0–9.5 m/s full sail [A] | Not found [U] | metres [A] |
| Skyrim | ~37 km² (14.3 mi²) explorable [C]; heightmap 119 × 94 cells, 1 cell ≈ 57.6 m [C] | 9 holds; ~4 km² each, ~2 km across [U, derived] | Throat of the World ≈ 766 m in-game (Z 39,655 vs sea level Z −14,000) [C] | Not verified [U] | Not verified [U] | ~64 units ≈ 1 yard (~1.43 cm/unit) [C] |
| The Witcher 3 | Official slide: Novigrad/Velen 8.5 × 8.5 km (72 km²), Skellige 8 × 8 km (64 km²), total ~136 km² (incl. sea) [A]; Reddit measured playable: Velen/Novigrad 15.2 km², Skellige 28.9 km² [C] | Each map is a region; sub-areas of a few km [U] | Not found [U] | Not found [U] | Not found [U] | metres (assumed) [U] |
| Zelda: BotW | Community: ~9 × 6.8 km playable (~60–70 km²) [C]; popular "12 × 10 km" figure not confirmed | ~15 named regions; ~4–5 km² each [U, derived] | Not found [U] (Mt. Lanayru height not found) | Not found [U] | Long (landmarks visible across map) [U] | metres (game HUD) [A] |
| Zelda: TotK | Surface ≈ BotW; Depths ≈ same footprint as surface; Sky = scattered islands [A] | as BotW | Sky islands to ~3,000 m, altitude cap ~3,300 m [C]; Starting Sky Island 1,610 m [C] | Not found [U]; gravity measured ≈ 28.2 m/s² (≈3× Earth) [C] | — | metres (HUD) [A] |
| Red Dead Redemption 2 | Estimates disagree: ~12 mi² (31 km²) to ~21 mi² playable (54 km²); 30 mi² (78 km²) in another list [C] | 5 states; regions a few km across [U] | Not found [U] | Gallop speed not published (stats on 1–10 scale) [A] | Not found [U] | metres (assumed) [U] |
| Elden Ring | Lands Between ≈ 79 km² (30.5 mi²) [C]; ~88 km² incl. seas, walkable < ¼ of that [C] | ~6 major regions (Limgrave, Liurnia…); ~10 km² each [U, derived] | Not found [U] | Torrent speed not found [U] | Not found [U] | metres (assumed) [U] |
| No Man's Sky | Planets; radius claims ~130 km (forum, if 1 unit = 1 m) vs much smaller if 1 unit ≈ 3 m [C] | One biome archetype per whole planet [A] | Not found [U] | Not found [U] | Not found [U] | disputed [C] |
| MS Flight Simulator 2020/2024 | Whole Earth (510 M km²) [A] | Real ecoregions | Real Everest 8,849 m | Aircraft speeds | Not found [U] | metres; Bing aerial + elevation, ~400 photogrammetry cities [A] |
| Enshrouded | Early Access ~24 km² (≈ 8 × 3 km playable; 8 × 8 km world grid) [A]; 1.0: 64 km² planned [A], 72 km² fan estimate [C] | ~7 hand-made biomes in fixed order; ~3 km across each [U, derived] | Not found [U] | Not found [U] | Not found [U] | voxels (size not found) [U] |
| **Earth (reference)** | 149 M km² land [U] | Ecoregion mean ~150,000 km², median 56,300 km² (867 ecoregions) [V] | Everest 8,849 m; Alps 4,809 m [V] | Walk ~1.4 m/s [A]; horse gallop 9–15 m/s [A] | Horizon 4.7 km at 1.7 m eye height [V] | metres |

---

## 2. Per-game notes

### Minecraft (Java Edition)
- **World border** at about ±29,999,984 blocks from origin (diameter 59,999,968); hard coordinate limit 30,000,000. [V] https://minecraft.wiki/w/World_border
- **Build range since 1.18**: Y −64 to 319, 384 blocks total; sea level Y 63. [V] https://minecraft.wiki/w/Java_Edition_1.18
- **Terrain height**: mountains reach up to about Y 256 (some say ~260); Y 320 is the build limit, not the terrain cap. Many "jagged peaks" stay well below this. [A] https://sportskeeda.com/minecraft/how-tall-can-mountains-minecraft-s-1-18-update ; https://sportskeeda.com/minecraft/news-minecraft-1-18-terrain-build-limits-players-revealed
  - So highest relief ≈ 190 blocks above sea level; from Y −64 floor to peak ≈ 320 blocks.
- **1.18 terrain change**: terrain generation decoupled from biome generation; biomes chosen by multi-noise (temperature, vegetation/humidity, continentalness, erosion, weirdness/ridges, depth). In Large Biomes the multinoise is slowed ×4 for temperature, vegetation, continentalness and erosion (not ridges) → most biomes ~4× larger. [V] https://minecraft.wiki/w/Java_Edition_1.18
- **Biome size**: no official width. Snapshot 21w40a reportedly "doubled the size of most biomes on average". [A] https://sportskeeda.com/minecraft/minecraft-1-18-snapshot-21w40a-brings-bigger-biomes . Pre-1.18 layer-stack generation described at https://minecraft.fandom.com/wiki/Biome/Before_1.18 . Typical width a few hundred to ~1,000+ blocks: [U] (player experience; no source found).
- **Speeds**: walk 4.317 m/s, sprint 5.612 m/s (sprint 30% faster). [V] https://minecraft.wiki/w/Sprinting . Horse speeds not found on that page [U] (commonly quoted ~4.7–14.2 m/s depending on horse; unverified).
- **Render distance**: 2–32 chunks (32 needs ≥1 GB RAM, else 16); defaults 16 (Fancy), 8 (Fast), 32 (Fabulous) per wiki text; simulation distance min 5. 1 chunk = 16 blocks → max 512 m. [V] https://minecraft.wiki/w/Options

### Valheim
- **World**: land generated to 10,000 m radius; sea floor drops between 10,000 and 10,500 m (total 10,500). [A] Expand World docs: https://thunderstore.io/c/valheim/p/JereKuusela/Expand_World/v/1.4.0 ; generator summary: https://metabot.gg/en/valheim/map
- **Biome bands** (distance from centre, from generator, per metabot.gg) [A]:
  - Meadows: within 5,000 m (fills gaps)
  - Black Forest: 600–6,000 m (noise 0.4)
  - Swamp: 2,000–6,000 m (noise 0.6, height 0.05–0.25)
  - Plains: 3,000–8,000 m (noise 0.4)
  - Mistlands: 6,000–10,000 m (noise 0.4)
  - Mountain: wherever normalised height > 0.4 (suppressed near centre)
  - Ocean: height ≤ 0.02
  - Ashlands / Deep North: beyond 12,000 m of a centre offset 4,000 m south / north
  - Minimum distances vary by ±100 m with bearing.
- **Biome patch size**: no measured figure. Bands are thousands of metres wide but patches within them are broken up by noise; roughly 0.5–2 km across from map viewing. [C/U]
- **Heights**: water level 30 m (mod docs on vanilla). [A] Max mountain height in metres not found. [U]
- **Speeds**: walk 4 m/s, run 7–8.75 m/s (running skill), sneak 2, swim 2, slow walk 1.6, ~5 m/s running in water ≥ 0.4 m. [A] https://gamever.io/knowledge-base/movement-speed-noise-in-valheim-the-numbers-behind-sneaking-and-running (quotes the community wiki; note https://valheim.fandom.com/wiki/Movement_speed snippet says "5 m/s at running skill 0", so sources conflict [C])
- **Boats**: Raft 2.5–3.1 m/s full sail; Karve 3.9–7.0; Longship 5.04–9.5 m/s full sail, 3.16 paddling. [A] https://www.bisecthosting.com/blog/valheim-boats-ships-stats-crafting-materials-raft-karve-longship-drakkar
- **View distance**: not found. [U]

### Skyrim
- **Area**: ~37 km² (14.3 mi²) is the common figure; one forum post gives 11.7 mi². Worldspace heightmap ~119 × 94 cells, cell = 57.6 m. [C] https://forums.nexusmods.com/topic/669051-distances-in-skyrim ; https://cyberpost.co/how-big-is-skyrim-in-game/
- **Throat of the World**: summit Z 39,655, sea level Z −14,000 (Creation Kit) → 53,655 units ≈ 766 m at ~1.43 cm/unit. [C] https://content2.uesp.net/wiki/Skyrim_talk:Throat_of_the_World . A fan "×10" scaling to ~7,665 m is lore, not geometry.
- **Units**: 64 units ≈ 1 yard (recollection, not official). [C] https://steamcommunity.com/app/72850/discussions/0/613940477917641227
- Speeds and view distance: not verified. [U]

### The Witcher 3
- 2014 slide: Novigrad region 8.5 × 8.5 km, Skellige 8 × 8 km; ~136 km² total; with Toussaint ~142 km² (these include sea and unplayable area). [A] https://www.dualshockers.com/the-witcher-3-two-of-the-areas-are-over-52-square-miles-by-themselves-3-5-times-larger-than-skyrim/ ; https://gamingbolt.com/witcher-3-map-size-compared-to-gta5-skyrim-far-cry-4-new-screens-show-different-visual-settings
- Measured playable: Velen/Novigrad (+HoS) 15.2 km², Skellige incl. ocean 28.9 km². [C] https://twinfinite.net/ps4/this-console-generations-8-largest-open-world-maps/14
- Heights, speeds, view distance: not found. [U]

### Breath of the Wild / Tears of the Kingdom
- BotW playable ~9 × 6.8 km (Reddit measurement); fan estimates 60–70 km²; "360 km²" came from a leaked map image and is not reliable. Nintendo said only "12× Twilight Princess". [C] https://www.mic.com/articles/170556/how-big-is-the-map-in-zelda-breath-of-the-wild-bigger-than-skyrim-and-witcher-3 ; https://www.giantbomb.com/forums/the-legend-of-zelda-breath-of-the-wild-231214/breath-of-the-wilds-known-overworld-map-size-estim-1798562/
- The often-quoted "12 × 10 km" figure: **not confirmed** by any source found. [U]
- TotK: Depths share the surface footprint; sky is the smallest by explorable area. [A] https://screenrant.com/zelda-tears-kingdom-underground-map-size-totk/ ; https://primagames.com/tips/how-big-is-the-tears-of-the-kingdom-map-vs-breath-of-the-wild-answered
- TotK altitudes (game HUD): highest island ~3,000 m, altitude cap ~3,300 m, Starting (Great) Sky Island 1,610 m. [C] https://www.ginx.tv/en/how-high-can-you-go
- TotK gravity measured ≈ 28.2 m/s², ~3× Earth: a sign the game's "metres" are not real metres. [C] https://automaton-media.com/en/news/20230801-20421/
- Link's run/sprint speed and horse speed: not found. [U]

### Red Dead Redemption 2
- Size estimates conflict: ~12 mi² (~31 km²), ~21 mi² playable (~54 km²), 30 mi² (~78 km²). [C] https://twinfinite.net/ps4/this-console-generations-8-largest-open-world-maps/18/ ; https://expertbeacon.com/is-elden-ring-map-bigger-than-rdr2/
- Horse stats are on a 1–10 scale; no m/s published. [A] https://screenrant.com/red-dead-redemption-2-horses-ranked/ . Cross-map times not found. [U]
- The "~75 km²" figure in the brief is consistent only with the 30 mi² estimate. [C]

### Elden Ring
- Lands Between ≈ 79 km² (30.5 mi²), Reddit measurement using Torrent's length and a bridge as scale. [C] https://www.ggrecon.com/guides/elden-ring-map-size-comparison ; https://pcgamer.com/games/rpg/elden-ring-geographer-tests-rigorous-calculation-against-weed-fueled-horse-math-to-determine-the-exact-size-of-the-lands-between (article body not retrievable)
- Wiki: ~88 km² incl. seas, walkable area under a quarter of that. [C] https://eldenring.wiki.gg/wiki/Lands_Between
- Heights, Torrent speed: not found. [U]

### No Man's Sky
- One biome archetype per planet. [A] https://nms.miraheze.org/wiki/Biome
- Radius: forum estimate ~130 km assuming 1 unit = 1 m; another source says 1 unit ≈ 3 m. Unresolved. [C] https://steamcommunity.com/app/275850/discussions/0/2952595757891767014

### Microsoft Flight Simulator 2020/2024
- Whole Earth; Bing aerial imagery and elevation as base (SimFlight lists Bing textures/elevation "LOD 19"); ~400 photogrammetry cities (>1,000 photogrammetry locations total); rest procedural. [A] https://techcrunch.com/2020/08/17/meet-the-startup-that-helped-microsoft-build-the-world-of-flight-simulator ; https://www.simflight.com/2019/10/03/microsoft-flight-simulator-facts-and-features/ ; Asobo terrain talk: https://WWW.ASOBOSTUDIO.COM/files/inline-images/Designing_Terrain_System_Fuentes_Lionel.pdf (not read)
- Elevation mesh resolution: forum users call it coarse vs 10 m DEMs; no official figure. [C] https://forums.flightsimulator.com/t/disappointing-mountain-terrain-in-the-alps-and-many-other-areas/186915?page=3
- "2 petabytes": unconfirmed. [U]

### Enshrouded
- Early Access ~24 km², about 8 km × 3 km playable; developer comment says the world grid was 8 × 8 km, extended by 1 km per border. [A] https://videogames.si.com/guides/enshrouded-map-size ; https://estnn.com/enshrouded-map-size-everything-you-need-to-know
- 1.0: developers planned 64 km² [A]; fans estimate 72 km² [C]. The brief's "~1,024 km²" is **not supported** by anything found.
- Biomes hand-made in fixed order from Springlands to Cinder's Cradle. [A] https://primagames.com/tips/all-enshrouded-biomes-guide

---

## 3. Earth references

| Quantity | Value | Source | Conf. |
|---|---|---|---|
| Alps highest peak | Mont Blanc 4,809 m; 82 peaks > 4,000 m | https://en.wikipedia.org/wiki/Alps | V |
| Alps length × width | ~1,200 km × ~200–250 km | same | V |
| Himalaya highest | Everest 8,848.86 m | https://en.wikipedia.org/wiki/Himalayas | V |
| Himalaya length × width | ~2,400 km × 350 km (west) to 151 km (east) | same | V |
| Hill vs mountain | Hills commonly < 300–600 m; Soviet def: hill relative height ≤ 200 m | https://en.wikipedia.org/wiki/Hill | V |
| Glacial U-valley example | Yosemite Valley 12 km long, ~1.6 km wide, 910–1,070 m deep | https://en.wikipedia.org/wiki/Yosemite_Valley | V |
| Valley widths generally | V-valleys: tens to hundreds of m at floor; large glacial troughs 1–10 km | — | U |
| 1st-order stream width | Most abundant headwater width 32 ± 7 cm | https://lamb.caltech.edu/documents/19688/s41467-018-02991-w.pdf (Allen et al. 2018, Nature Communications) | A |
| Stream-order zones | Headwaters orders 1–3, mid-reaches 4–6, large rivers > 6 (River Continuum Concept) | https://en.wikipedia.org/wiki/River_Continuum_Concept | A |
| Order 4–6 widths | ~5–50 m typical; order 7+ ~ 50 m to hundreds of m | — (no table found; see Leopold & Maddock 1953, USGS PP 252; Downing et al. 2012, Inland Waters 2:229) | U |
| Global river area by order | Orders 5–9 make up most river *area*; 1st order dominates *length* | https://scholars.unh.edu/nh_epscor/231 (Downing et al. 2012) | A |
| Amazon width | 0.7 km (upper, min) – 3 km (middle avg) – 5 km (lower avg) – 10–14 km max; estuary 340 km | https://en.wikipedia.org/wiki/Amazon_River | V |
| Ecoregion size | 867 ecoregions; mean ~150,000 km², median 56,300 km² (≈ 240 km square) | Olson et al. 2001, BioScience 51:933, PDF https://www.cbd.int/doc/articles/2002-/A-00513.pdf | V |
| Udvardy biotic provinces | mean 740,000 km², median 306,000 km² | same | V |
| Human walking speed | ~1.42 m/s preferred (studies range ~1.1–1.4) | https://en.wikipedia.org/wiki/Preferred_walking_speed ; https://pmc.ncbi.nlm.nih.gov/articles/PMC10884095 | A |
| Horse gaits | walk 1.4–1.8 m/s; trot 2.5–6.5; canter ~4.4–7.5; gallop ~11–13 avg (9–15 range) m/s | https://en.wikipedia.org/wiki/Horse_gait ; https://en.wikipedia.org/wiki/Canter_and_gallop | A |
| Horizon distance | d ≈ 3.57·√h km (h in m) → 4.7 km at 1.7 m; ~3.86·√h with refraction (~5.0 km) | https://en.wikipedia.org/wiki/Horizon | V |

Forest patch scale: no single source found. Landscape-ecology work treats stands at ~1–100 ha (100 m–1 km) and landscapes at 10–1,000 km². [U]

---

## 4. Scale compression: what sources say

- **Ubisoft, AC Odyssey** (world director Benjamin Hall): the team "needed to play about with scale"; world "much smaller than real life" but keeping a sense of depth, with shorter distances so getting around stays fun; cartographic generalisation of real Greece. [A] https://news.ubisoft.com/en-us/article/348038/assassins-creed-odyssey-the-game-that-greece-built ; https://www.ubisoft.com/en-us/game/assassins-creed/news/2zOk4Z7OoI5COWAh6NRsfS/inside-the-studio-with-world-director-benjamin-hall . Map measured ~16 × 16 km (~256 km²) vs. real Greece ~132,000 km², i.e. area ratio ~1:500, linear ~1:20–1:25. [C] https://twinfinite.net/news/how-big-assassins-creed-odysseys-map-compared-greece/
- **Pacing by content, not size** (Game Informer roundtable): Sucker Punch (Ghost of Tsushima) spaces content so the player is rewarded after "a minute or two" of wandering and uses a visible landmark forest seen "from across the island"; Insomniac tuned swing speed and building spacing so a big building passes every couple of swings and exaggerated building heights; Ubisoft placed landmarks first, then filled in. [A] https://www.gameinformer.com/2021/05/20/more-than-just-dots-on-a-map-the-challenges-behind-open-world-game-creation
- **Space compression** as a recognised trope: settlements minutes apart; map read as travel time not distance. [C] https://tvtropes.org/pmwiki/pmwiki.php/Main/SpaceCompression
- **Big worlds feel empty** when content repeats; perceived scale tracks density and variety, not km². [A] https://gamedeveloper.com/game-platforms/escaping-escapism-the-impact-of-digital-world-scale-on-realism-in-games ; https://www.pcgamer.com/size-isnt-everything-in-open-world-games
- **Vertical scale is also compressed/distorted**: Skyrim's highest mountain is ~766 m (real alpine peaks 4–5 km); Minecraft peaks ~190 m above sea; TotK uses ~3× Earth gravity so falls from 3 km sky islands fit the pacing. [C]
- **Pattern across sources** (my synthesis): objects, buildings and characters stay at real size; horizontal distances shrink ~10–25× linear (AC Odyssey ~1:20; Skyrim/Elden Ring regions of a few km standing in for provinces hundreds of km across); vertical relief shrinks ~5–10× (Skyrim 766 m vs 4–5 km); run speeds are ~1.5–2× real jogging speed (5.6–8.75 m/s vs ~3 m/s jog); content spaced at 1–2 min of travel. [U, synthesis]

---

## 5. Derived: time to cross a typical biome/region at run speed

All rows [U] unless both inputs are verified. "Across" is the side of a square of the given area, or a quoted width.

| Game | Typical biome/region across | Run speed | Crossing time |
|---|---|---|---|
| Minecraft | ~300–1,000 m (est.) | 5.6 m/s sprint [V] | ~1–3 min |
| Valheim | ~0.5–2 km patch (est.) | 7–8.75 m/s [A] | ~1–5 min |
| Skyrim | ~2 km per hold (37 km² / 9) | ~5–6 m/s [U] | ~6 min |
| Witcher 3 | Velen playable ~4 km (15.2 km²) | not found | ~10 min at 6 m/s [U] |
| Elden Ring | ~3 km per region (79 km² / ~7) | Torrent not found | ~4–5 min at 12 m/s [U] |
| Enshrouded | ~3 km per biome (64 km² / 7) | not found | ~8 min at 6 m/s [U] |
| Earth ecoregion (median) | ~240 km (√56,300 km²) [V] | 1.4 m/s walk | ~47 h walking; ~5 h at 13 m/s gallop |
| Earth, Alps cross-width | ~200–250 km [V] | 1.4 m/s | ~40–50 h walking |

Takeaway: games tune biome/region crossings to roughly 1–10 minutes of travel, while a median real ecoregion is ~2 days of walking, i.e. games are ~300–1,000× faster to cross a "biome", from both smaller regions (~50–100× linear) and faster movement (~4–6×).

---

## Unverified / open items
- Minecraft typical biome width in blocks (pre- vs post-1.18): no measured source.
- Valheim max mountain height (m) and view distance.
- BotW "12 × 10 km", Link and horse speeds, Mt. Lanayru height.
- RDR2 horse gallop m/s and agreed map area.
- Skyrim run speed; any game's draw distance besides Minecraft.
- Elden Ring Torrent speed.
- No Man's Sky planet radius (unit scale disputed).
- MSFS elevation mesh resolution and draw distance.
- Enshrouded 1.0 final area; "~1,024 km²" not supported.
- River widths for Strahler orders 2–6: no numeric table found.
