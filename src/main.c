#include "kos.h"

#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <linux/kd.h>
#include <fcntl.h>
#include <unistd.h>

static volatile sig_atomic_t stop;

static void on_signal(int signal_number)
{
    (void)signal_number;
    stop = 1;
}

static void frame(State *s, Inputs *inputs)
{
    inputs_poll(inputs, s);
    ui_tick(s);
    if (s->redraw) {
        ui_draw(s);
        display_present(s->d);
        s->redraw = false;
    }
}

static void set_tty(int console, int mode)
{
    if (console >= 0) ioctl(console, KDSETMODE, mode);
}

int main(int argc, char **argv)
{
    static State state;
    Display display;
    if (argc == 2 && !strcmp(argv[1], "--prepare-accounts")) return account_bootstrap() ? 0 : 1;
    if (argc == 2 && !strcmp(argv[1], "--network-agent")) return network_agent();
    if (argc == 2 && !strcmp(argv[1], "--self-test")) return ui_test();
    if (argc == 5 && !strcmp(argv[1], "--render")) {
        if (!display_memory(&display, atoi(argv[2]), atoi(argv[3]))) return 1;
        ui_init(&state, &display, NULL);
        ui_draw(&state);
        bool saved = display_dump(&display, argv[4]);
        display_close(&display);
        return saved ? 0 : 1;
    }
    if (argc != 1) return 2;
    const char *fb = getenv("KOS_FB");
    const char *data = getenv("KOS_DATA");
    if (!display_open(&display, fb && *fb ? fb : "/dev/fb0")) {
        fprintf(stderr, "KreativeOS: no usable framebuffer at %s\n", fb ? fb : "/dev/fb0");
        return 1;
    }
    int console = open("/dev/tty1", O_RDWR | O_CLOEXEC);
    set_tty(console, KD_GRAPHICS);
    struct sigaction action = { .sa_handler = on_signal };
    sigemptyset(&action.sa_mask);
    sigaction(SIGTERM, &action, NULL);
    sigaction(SIGINT, &action, NULL);
    Inputs inputs;
    if (!inputs_open(&inputs)) fprintf(stderr, "KreativeOS: input hotplug monitor unavailable\n");

    if (getenv("KOS_APPLIANCE") && geteuid() == 0) {
        ui_init(&state, &display, NULL);
        state.app = AUTH;
        state.login.setup = account_first_run();
        state.login.add = getenv("KOS_ADD_ACCOUNT") && !state.login.setup;
        ui_draw(&state);
        display_present(&display);
        fprintf(stderr, "KreativeOS greeter (%dx%d)\n", display.w, display.h);
        while (!stop && !state.login.done && state.running) frame(&state, &inputs);
        if (!state.login.done) return state.exit_code;
        uid_t uid = state.login.uid;
        pid_t session = fork();
        if (session < 0) return 1;
        if (session > 0) {
            inputs_close(&inputs);
            display_close(&display);
            int result = 0;
            while (waitpid(session, &result, 0) < 0 && errno == EINTR)
                if (stop) kill(session, SIGTERM);
            account_cleanup(uid);
            set_tty(console, KD_TEXT);
            if (console >= 0) close(console);
            return WIFEXITED(result) ? WEXITSTATUS(result) : 1;
        }
        if (!account_enter(&state)) _exit(1);
        char home[PATH_MAX], username[33];
        strcpy(home, state.login.home);
        strcpy(username, state.login.user);
        memset(&state, 0, sizeof(state));
        ui_init(&state, &display, home);
        strcpy(state.user, username);
    } else {
        ui_init(&state, &display, data && *data ? data : "/data");
    }

    ui_draw(&state);
    display_present(&display);
    fprintf(stderr, "KreativeOS ready (%dx%d)\n", display.w, display.h);
    while (!stop && state.running) frame(&state, &inputs);
    notes_save(&state);
    terminal_close(&state.term);
    inputs_close(&inputs);
    if (!getenv("KOS_APPLIANCE")) set_tty(console, KD_TEXT);
    if (console >= 0) close(console);
    display_close(&display);
    return stop ? 0 : state.exit_code;
}
