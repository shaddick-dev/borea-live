# Borea design direction

**One language, everywhere.** A design-language change applies to the whole project in the same change: every card, the step rail, dialogs, facts, the terminal app's equivalents and these notes. Nothing keeps an older look. If a surface can't take part (a terminal has no hover), say what it does instead here.

## Palette and motion

- Nord Polar Night surfaces and frost/ice blue highlights. No purple, violet, lavender or pink UI accents, including stock controls.
- Dark by default; the light appearance uses Nord Snow Storm and cool blue accents.
- Keep upstream logo artwork in its original colours. No geometric frames behind logos and no game-stat panels.
- Wind moves from right to left in irregular gusts of hard, straight streaks. Each eases in, accelerates with a motion blur that stretches with its speed, then drags to a crawl, fading in and out so it never visibly starts or stops. Page changes smoothly accelerate it, then it settles back to a calm pace.
- Pause stops all decorative motion. Every state still shows, without animation. Avoid flashing effects and layout shifts.

## Cards

Every card in the GUI is the same card in four sizes: hero (families and distributions), compact (every other choice), rail (the steps) and form (fields to fill in). All have 16 px corners.

- **Accent.** A family card wears its family's primary colour (Arch #1793D1, Fedora #51A2DA, Debian #A80030, openSUSE #73BA25). A distribution card wears its own brand colour (Arch Linux #1793D1, CachyOS #00AA88 from its mark), Borea's neutral grey where its mark is monochrome (Omarchy), or its family's until reviewed. Everything else wears Borea's ice.
- **Hover.** The card rises (4 px hero, 2 px compact and rail) with a deeper shadow and a glow of its accent, and the side under the pointer gives way into shade, drawn beneath the words so logos and text never dim.
- **Picked.** A soft gradient of the accent fades in and drifts slowly, with a 2 px rim and a gently breathing glow; just picked, a faint bloom grows from the pointer. No shimmer, glint, cursor glow or "SELECTED" label.
- **Can't be picked.** Greyed toward the background, still, with a quiet tag at the bottom right saying why: "Coming soon", "Not available" or "Included" (included things also show as picked).
- **One place for extras.** System extras (Gaming, Media codecs, Flatpak, Printing) are chosen only on the review: at the top of the Extra apps form, or as compact cards on a hand-off's review. An app an extra installs shows there as picked with a faint "in Gaming", never as a second choice.
- **Facts.** Facts to read (the machine, the final review) sit on flat cards: no hover, no greying.
- **Forms.** Fields to fill in (Account, Region, Extra apps) sit on titled form cards: a 15 px semibold title, sections split by full-width hairlines, options in a sunk band with switches at the end of the card. Each field is a faint hint on a thin line, never a box and never a pre-filled example; the line and its card's rim turn ice while it is the one being filled in. Drop-downs open with a chevron and indent their rows in the same sunk band, a round selector on the left. The only words a form adds are its errors, inline under what they're about, the password's strength and whether passwords match.

## Words

- Hero cards carry the project's own words, quoted and credited (its published line or its founder's), never invented taglines, and the release type along the bottom in the same words for a family and its flagship ("Rolling", "Fresh stable", "Trixie (v13.*)"). Status tags sit at the bottom right.
- Each family's name uses its own type where it publishes a free face (Fedora: Comfortaa; openSUSE: Source Sans 3; quotes in Open Sans). Otherwise Noto Sans.
- Sentence case throughout: "Step 4 of 6", not tiny tracked capitals.

## Terminal

- The terminal app (borea-rataTUI) shares the palette. Its picked row is a soft ice tint, its version of the picked wash; it has no hover, so it has no lift or push.

These are the user's saved preferences and supersede the earlier purple Material palette.
