#include "kos.h"

#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <linux/kd.h>
#include <fcntl.h>
#include <unistd.h>

static volatile sig_atomic_t stop;

static void on_signal(int signal_number)
{
    (void)signal_number;
    stop = 1;
}

int main(int argc, char **argv)
{
    static State state;
    Display display;
    if (argc == 2 && strcmp(argv[1], "--self-test") == 0) return ui_test();
    if (argc == 5 && strcmp(argv[1], "--render") == 0) {
        if (!display_memory(&display, atoi(argv[2]), atoi(argv[3]))) return 1;
        ui_init(&state, &display, NULL);
        ui_draw(&state);
        bool saved = display_dump(&display, argv[4]);
        display_close(&display);
        return saved ? 0 : 1;
    }
    if (argc != 1) {
        fprintf(stderr, "Usage: %s [--self-test | --render WIDTH HEIGHT PATH]\n", argv[0]);
        return 2;
    }
    const char *fb = getenv("KOS_FB");
    const char *data = getenv("KOS_DATA");
    if (!display_open(&display, fb && *fb ? fb : "/dev/fb0")) {
        fprintf(stderr, "KreativeOS: no usable framebuffer at %s\n", fb ? fb : "/dev/fb0");
        return 1;
    }
    int console = open("/dev/tty1", O_RDWR | O_CLOEXEC);
    if (console >= 0) ioctl(console, KDSETMODE, KD_GRAPHICS);
    struct sigaction action = { .sa_handler = on_signal };
    sigemptyset(&action.sa_mask);
    sigaction(SIGTERM, &action, NULL);
    sigaction(SIGINT, &action, NULL);
    Inputs inputs;
    if (!inputs_open(&inputs)) fprintf(stderr, "KreativeOS: input hotplug monitor unavailable\n");
    ui_init(&state, &display, data && *data ? data : "/data");
    ui_draw(&state);
    display_present(&display);
    fprintf(stderr, "KreativeOS ready (%dx%d)\n", display.w, display.h);
    while (!stop && state.running) {
        inputs_poll(&inputs, &state);
        ui_tick(&state);
        if (state.redraw) {
            ui_draw(&state);
            display_present(&display);
            state.redraw = false;
        }
    }
    ui_key(&state, KEY_SAVE_UI);
    if (state.term.pid > 0) kill(-state.term.pid, SIGTERM);
    inputs_close(&inputs);
    if (console >= 0) {
        ioctl(console, KDSETMODE, KD_TEXT);
        close(console);
    }
    display_close(&display);
    return stop ? 0 : 42;
}
