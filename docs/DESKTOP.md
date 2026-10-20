# IndianOS desktop roadmap

## Objective

A familiar, original, keyboard-friendly desktop designed for Indian users. Familiar workflows are allowed; proprietary Windows source, logos and artwork are not.

## Experience principles

- English and Hindi are first-class at the product level; Unicode, Indic input, font fallback, screen-reader hooks and keyboard navigation must be designed in early.
- Start menu equivalent: **Apps launcher**, with search and recently opened files.
- Shell: taskbar, notification center, window switcher, desktop and settings.
- File manager, terminal, text editor, browser integration and app installation come later.
- A minimal, light, accessible default theme is more important than visual effects.

## Architecture

```text
User Applications -> Window protocol -> IndianOS compositor
                                      -> Surface management / input routing
                                      -> Kernel graphics + input interfaces
```

The first compositor can be software-rendered into a mapped framebuffer with a simple shared-buffer protocol. Later GPU acceleration should use a separately designed driver interface. Do not claim Wayland/Linux binary compatibility simply because we borrow their architectural concepts.

## Suggested staged deliverables

1. Pixel drawing (during kernel boot; **not** a window manager).
2. User-space framebuffer demo once isolated processes exist.
3. Input events and cursor.
4. Two independent application windows with focus and composition.
5. Launcher, taskbar, accessible navigation, font and text shaping.
6. Settings, localization, packaging and secure updates.

## Early UX mock screens

- Sign in / choose language
- Desktop and launcher
- File manager
- Settings and network
- About IndianOS / updates

A marketing mockup is not implementation evidence. Each real UI screen needs functioning user-space services, API boundaries and tests.

## Windows compatibility

Native IndianOS applications are the primary target. The Windows ABI/driver ecosystem is proprietary and complex; ordinary .exe files **will not run by default**. Investigate source-level ports, web apps, virtual machines or a future compatibility layer only after a stable userspace ABI exists.
