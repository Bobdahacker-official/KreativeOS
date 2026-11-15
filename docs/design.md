# Desktop design

The UI uses a deep navy workspace, mint highlights, compact taskbar and consistent app chrome. Every window has the same title, minimize and close positions. Color identifies focus; text remains the primary label. Files, settings and packages use familiar list and command patterns.

Research references:

- Microsoft Windows design basics: https://learn.microsoft.com/en-us/windows/apps/design/basics/
- Navigation principles: https://learn.microsoft.com/en-us/windows/apps/design/basics/navigation-basics
- UEFI Graphics Output Protocol: https://uefi.org/specs/UEFI/2.10/12_Protocols_Console_Support.html

These references informed grouping, navigation consistency and framebuffer discovery. The desktop renderer, font glyph data, window management and app code are original Rust implementations in this repository. No Windows UI code or assets are included.
