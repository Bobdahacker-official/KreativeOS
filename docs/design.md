# Desktop design

The desktop uses a layered gradient wallpaper, a translucent system strip and dock, and softly shadowed windows. Original icons keep the main tools one click away, and hovering shows each label. Window controls live on the left, with title-bar dragging and a separate grid launcher.

The renderer draws into a RAM framebuffer. It composites rounded surfaces, shadows and text with alpha blending. Open Sans is rasterized into a compressed grayscale glyph atlas at four sizes. Its license is in [assets/OpenSans-LICENSE.txt](../assets/OpenSans-LICENSE.txt).

The palette, app marks and wallpaper are drawn by KreativeOS. Firmware graphics access follows the [UEFI Graphics Output Protocol](https://uefi.org/specs/UEFI/2.10/12_Protocols_Console_Support.html).
