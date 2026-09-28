WaveSink - icon pack
================

App ID: us.echo.WaveSink

Layout (freedesktop hicolor theme):
  scalable/apps/us.echo.WaveSink.svg              full-color SVG (use this everywhere it's supported)
  symbolic/apps/us.echo.WaveSink-symbolic.svg     monochrome, follows the panel/tray text color
  hicolor/<size>/apps/us.echo.WaveSink.png        rasters: 16, 24, 32, 48, 64, 128, 256, 512
  extras/us.echo.WaveSink-flat.svg                flat #5557e0 plate (no gradient)
  extras/us.echo.WaveSink-graphite.svg            dark plate, indigo mark
  extras/tray-white-22.png             tray glyph, light panels
  extras/tray-black-22.png             tray glyph, dark panels

Install (per-user):
  cp -r hicolor/*   ~/.local/share/icons/hicolor/
  cp scalable/apps/us.echo.WaveSink.svg  ~/.local/share/icons/hicolor/scalable/apps/
  gtk-update-icon-cache ~/.local/share/icons/hicolor

In your .desktop file:
  Icon=us.echo.WaveSink
