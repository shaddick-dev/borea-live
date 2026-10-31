# Borea design preference

For GUI or terminal styling, follow [docs/design.md](docs/design.md).
The user's preference is explicit: no purple, violet, lavender or pink UI accents.
Use Nord Polar Night surfaces, frost/ice blues, and right-to-left wind with smooth
page-transition acceleration. Preserve upstream logo artwork and installer guards.

**Keep the whole project cohesive.** When the user changes the design language,
apply it to every surface in the same change: every card (families, distributions,
networks, editions, desktops, extras, storage, apps), the step rail, dialogs,
facts, the terminal app where it can take part, and docs/design.md. Never leave a
surface on an older look. In the GUI, every card goes through `card_shell()`.
