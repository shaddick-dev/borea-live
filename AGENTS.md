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

# Visual previews and merging

This is the user's standing workflow for AI agents:

- Any change to appearance, layout, navigation, animation or a user-visible flow
  must include updated visual evidence. Cover every affected surface and state.
- Render the actual updated app using the safe showcase fixtures. Use a fresh
  capture directory and demonstrate changed hover, selection and transition states.
- Refresh `docs/assets/installer-showcase.mp4`, the looping GIF made from that
  video, and `installer-showcase.png`. Keep the README GIF clickable so it opens
  the full video player. Update other relevant diagrams/screenshots when needed.
- Follow the capture/encoding commands in [docs/iced-ui.md](docs/iced-ui.md).
  Inspect the images, decode both media files, and check the links before committing.
- Include previews in the same PR as the visual change. Resolve conflicts while
  preserving other contributors' work, run the relevant checks, and wait for CI.
- Once checks and repository requirements pass, automatically **squash and merge**
  the task's PR. This is authorized by the user; do not ask again for routine
  confirmation. Use GitHub auto-merge when appropriate, verify it actually merged,
  and report any required review or permission that prevents completion. Never
  bypass checks/reviews or merge unrelated PRs under this instruction.
