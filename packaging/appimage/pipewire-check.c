/* WaveSink links libpipewire, so on a system without it the dynamic loader
 * kills the process before main() and the app's own "can't reach PipeWire"
 * dialog never runs - the user just sees nothing. This runs first, from
 * AppRun, and explains the problem instead.
 *
 * It must not link libpipewire itself, hence dlopen rather than a direct call.
 */
#include <dlfcn.h>
#include <gtk/gtk.h>
#include <stdio.h>
#include <stdlib.h>

/* Overridable so CI can exercise the failure path without hiding a system
 * library from the loader. */
#define DEFAULT_SONAME "libpipewire-0.3.so.0"

static const char *const MESSAGE =
    "PipeWire was not found.\n\n"
    "WaveSink is a mixer for PipeWire, so it cannot run without it. Please "
    "install PipeWire - along with pipewire-pulse and WirePlumber 0.5 or "
    "newer - before running WaveSink.";

int main(int argc, char **argv) {
  const char *soname = getenv("WAVESINK_PIPEWIRE_SONAME");
  if (soname == NULL || *soname == '\0') {
    soname = DEFAULT_SONAME;
  }

  void *handle = dlopen(soname, RTLD_LAZY);
  if (handle != NULL) {
    dlclose(handle);
    return 0;
  }

  /* Always say it on stderr: the dialog needs a display, a terminal does not. */
  fprintf(stderr, "wavesink: %s cannot be loaded - PipeWire is not installed\n",
          soname);

  if (gtk_init_check(&argc, &argv)) {
    GtkWidget *dialog = gtk_message_dialog_new(
        NULL, 0, GTK_MESSAGE_ERROR, GTK_BUTTONS_OK, "%s", MESSAGE);
    gtk_window_set_title(GTK_WINDOW(dialog), "WaveSink can't start");
    gtk_dialog_run(GTK_DIALOG(dialog));
    gtk_widget_destroy(dialog);
  }
  return 1;
}
