WaveSink - icon pack
================

App ID: us.echo.Sink

Layout (freedesktop hicolor theme):
  scalable/apps/us.echo.Sink.svg              full-color SVG (use this everywhere it's supported)
  symbolic/apps/us.echo.Sink-symbolic.svg     monochrome, follows the panel/tray text color
  hicolor/<size>/apps/us.echo.Sink.png        rasters: 16, 24, 32, 48, 64, 128, 256, 512
  extras/us.echo.Sink-flat.svg                flat #5557e0 plate (no gradient)
  extras/us.echo.Sink-graphite.svg            dark plate, indigo mark
  extras/tray-white-22.png             tray glyph, light panels
  extras/tray-black-22.png             tray glyph, dark panels

Install (per-user):
  cp -r hicolor/*   ~/.local/share/icons/hicolor/
  cp scalable/apps/us.echo.Sink.svg  ~/.local/share/icons/hicolor/scalable/apps/
  gtk-update-icon-cache ~/.local/share/icons/hicolor

In your .desktop file:
  Icon=us.echo.Sink
