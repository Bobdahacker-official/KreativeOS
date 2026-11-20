# Desktop design

The desktop uses jade and graphite, a full-width taskbar and a two-column launcher. App marks share a 20-pixel grid and two-pixel strokes. The same vector mark appears in the taskbar, launcher and window title bar. Window controls are on the right, and the active title bar has a jade underline.

The renderer draws into a RAM framebuffer. Fonts come from Open Sans, rasterized into a compressed grayscale glyph atlas at four sizes. Button labels and window titles use the visible glyph bounds for centering. The font license is in [assets/OpenSans-LICENSE.txt](../assets/OpenSans-LICENSE.txt).

KreativeOS draws its own app marks and wallpaper. Firmware graphics access follows the [UEFI Graphics Output Protocol](https://uefi.org/specs/UEFI/2.10/12_Protocols_Console_Support.html).
