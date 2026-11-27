#include "kos.h"
#include <vterm.h>

#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <pty.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/prctl.h>
#include <sys/wait.h>
#include <unistd.h>

static int save_row(int cols, const VTermScreenCell *cells, void *user)
{
    Terminal *t = user;
    VTermScreenCell *copy = malloc((size_t)cols * sizeof(*copy));
    if (!copy) return 0;
    memcpy(copy, cells, (size_t)cols * sizeof(*copy));
    int slot = (t->history_head + t->history_count) % TERM_HISTORY;
    if (t->history_count == TERM_HISTORY) t->history_head = (t->history_head + 1) % TERM_HISTORY;
    else t->history_count++;
    free(t->history[slot]);
    t->history[slot] = copy;
    t->history_cols[slot] = cols;
    return 1;
}

static int restore_row(int cols, VTermScreenCell *cells, void *user)
{
    Terminal *t = user;
    if (!t->history_count) return 0;
    int slot = (t->history_head + --t->history_count) % TERM_HISTORY;
    VTermScreenCell *old = t->history[slot];
    VTermColor fg, bg;
    vterm_state_get_default_colors(vterm_obtain_state(t->emulator), &fg, &bg);
    for (int i = 0; i < cols; i++) {
        cells[i] = i < t->history_cols[slot] ? old[i] :
            (VTermScreenCell){ .width = 1, .fg = fg, .bg = bg };
    }
    free(old);
    t->history[slot] = NULL;
    return 1;
}

static int cursor_move(VTermPos point, VTermPos old, int visible, void *user)
{
    (void)old;
    Terminal *t = user;
    t->cursor_row = point.row;
    t->cursor_col = point.col;
    t->cursor_visible = visible;
    return 1;
}

static void transmit(const char *bytes, size_t count, void *user)
{
    Terminal *t = user;
    while (t->fd >= 0 && count) {
        ssize_t written = write(t->fd, bytes, count);
        if (written < 0 && errno == EINTR) continue;
        if (written <= 0) return;
        bytes += written;
        count -= (size_t)written;
    }
}

bool terminal_start(State *s, int rows, int cols)
{
    Terminal *t = &s->term;
    struct winsize size = { .ws_row = rows, .ws_col = cols };
    if (t->emulator) {
        if (rows != t->rows || cols != t->cols) {
            t->rows = rows;
            t->cols = cols;
            vterm_set_size(t->emulator, rows, cols);
            if (t->fd >= 0) ioctl(t->fd, TIOCSWINSZ, &size);
        }
        return true;
    }
    t->rows = rows;
    t->cols = cols;
    t->emulator = vterm_new(rows, cols);
    if (!t->emulator) return false;
    vterm_set_utf8(t->emulator, 1);
    vterm_output_set_callback(t->emulator, transmit, t);
    t->screen = vterm_obtain_screen(t->emulator);
    vterm_screen_enable_altscreen(t->screen, 1);
    static const VTermScreenCallbacks callbacks = {
        .movecursor = cursor_move, .sb_pushline = save_row, .sb_popline = restore_row,
    };
    vterm_screen_set_callbacks(t->screen, &callbacks, t);
    VTermColor fg, bg;
    vterm_color_rgb(&fg, 0x28, 0x2f, 0x42);
    vterm_color_rgb(&bg, 0xff, 0xff, 0xff);
    vterm_state_set_default_colors(vterm_obtain_state(t->emulator), &fg, &bg);
    vterm_screen_reset(t->screen, 1);

    t->pid = forkpty(&t->fd, NULL, NULL, &size);
    if (t->pid == 0) {
        if (geteuid() == 0 &&
            (setgroups(0, NULL) || setgid(65534) || setuid(65534))) _exit(126);
        if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) < 0) _exit(126);
        const char *home = *s->data ? s->data : "/";
        if (chdir(home) < 0) _exit(126);
        char env_home[PATH_MAX + 5], env_user[44];
        snprintf(env_home, sizeof(env_home), "HOME=%s", home);
        snprintf(env_user, sizeof(env_user), "USER=%s", s->user[0] ? s->user : "local");
        char *env[] = { env_home, env_user, "PATH=/bin:/usr/bin:/sbin:/usr/sbin",
            "TERM=xterm-256color", "PS1=\\u@kreative:\\w\\$ ", "HISTFILE=/dev/null",
            "LANG=C.UTF-8", NULL };
        char *argv[] = { "sh", "-i", NULL };
        execve("/bin/sh", argv, env);
        _exit(127);
    }
    if (t->pid < 0) {
        int failure = errno;
        t->fd = -1;
        char error[144];
        snprintf(error, sizeof(error), "Could not open a pseudo-terminal: %s.\r\n", strerror(failure));
        fprintf(stderr, "KreativeOS: forkpty: %s\n", strerror(failure));
        vterm_input_write(t->emulator, error, strlen(error));
        return false;
    }
    fcntl(t->fd, F_SETFD, FD_CLOEXEC);
    int flags = fcntl(t->fd, F_GETFL);
    if (flags >= 0) fcntl(t->fd, F_SETFL, flags | O_NONBLOCK);
    return true;
}

void terminal_poll(State *s)
{
    Terminal *t = &s->term;
    if (t->fd >= 0) {
        char bytes[4096];
        for (int i = 0; i < 8; i++) {
            ssize_t size = read(t->fd, bytes, sizeof(bytes));
            if (size <= 0) {
                if (size == 0 || (errno != EINTR && errno != EAGAIN)) {
                    close(t->fd);
                    t->fd = -1;
                    const char closed[] = "\r\nSession ended. Tap New to open a shell.\r\n";
                    vterm_input_write(t->emulator, closed, sizeof(closed) - 1);
                    if (s->app == TERMINAL) s->redraw = true;
                }
                break;
            }
            vterm_input_write(t->emulator, bytes, (size_t)size);
            if (s->app == TERMINAL) s->redraw = true;
        }
        vterm_screen_flush_damage(t->screen);
    }
    if (t->pid > 0 && waitpid(t->pid, NULL, WNOHANG) == t->pid) t->pid = 0;
}

void terminal_close(Terminal *t)
{
    if (t->fd >= 0) close(t->fd);
    if (t->pid > 0) {
        kill(t->pid, SIGHUP);
        waitpid(t->pid, NULL, WNOHANG);
    }
    if (t->emulator) vterm_free(t->emulator);
    for (int i = 0; i < TERM_HISTORY; i++) free(t->history[i]);
    *t = (Terminal){ .fd = -1 };
}

void terminal_key(State *s, int key)
{
    Terminal *t = &s->term;
    if (t->fd < 0) return;
    s->scroll = 0;
    if (key >= KEY_CONTROL_UI && key < KEY_CONTROL_UI + 26) {
        vterm_keyboard_unichar(t->emulator, 'a' + key - KEY_CONTROL_UI, VTERM_MOD_CTRL);
        return;
    }
    VTermKey mapped = VTERM_KEY_NONE;
    switch (key) {
    case KEY_BREAK_UI: vterm_keyboard_unichar(t->emulator, 'c', VTERM_MOD_CTRL); return;
    case KEY_SAVE_UI: vterm_keyboard_unichar(t->emulator, 's', VTERM_MOD_CTRL); return;
    case KEY_ESCAPE_UI: mapped = VTERM_KEY_ESCAPE; break;
    case KEY_HOME_UI: mapped = VTERM_KEY_HOME; break;
    case KEY_LEFT_UI: mapped = VTERM_KEY_LEFT; break;
    case KEY_RIGHT_UI: mapped = VTERM_KEY_RIGHT; break;
    case KEY_UP_UI: mapped = VTERM_KEY_UP; break;
    case KEY_DOWN_UI: mapped = VTERM_KEY_DOWN; break;
    case KEY_DELETE_UI: mapped = VTERM_KEY_DEL; break;
    case '\b': mapped = VTERM_KEY_BACKSPACE; break;
    case '\n': mapped = VTERM_KEY_ENTER; break;
    case '\t': mapped = VTERM_KEY_TAB; break;
    default:
        if (key >= 32 && key <= 126) vterm_keyboard_unichar(t->emulator, key, VTERM_MOD_NONE);
        return;
    }
    vterm_keyboard_key(t->emulator, mapped, VTERM_MOD_NONE);
}

int terminal_history(const State *s) { return s->term.history_count; }

static uint32_t terminal_color(Terminal *t, VTermColor color, bool foreground, bool light)
{
    if (foreground && VTERM_COLOR_IS_DEFAULT_FG(&color)) return light ? 0x262d40 : 0xe8ebfa;
    if (!foreground && VTERM_COLOR_IS_DEFAULT_BG(&color)) return light ? 0xffffff : 0x1a1e2f;
    vterm_screen_convert_color_to_rgb(t->screen, &color);
    return (uint32_t)color.rgb.red << 16 | (uint32_t)color.rgb.green << 8 | color.rgb.blue;
}

void terminal_draw(State *s, Rect surface, int scroll)
{
    int cols = (surface.w - 22) / 9;
    int rows = (surface.h - 20) / 21;
    if (cols < 1 || rows < 1) return;
    terminal_start(s, rows, cols);
    Terminal *t = &s->term;
    if (!t->screen) return;
    scroll = scroll > t->history_count ? t->history_count : scroll;
    for (int row = 0; row < rows; row++) {
        int actual = row - scroll;
        int history = (t->history_head + t->history_count + actual) % TERM_HISTORY;
        for (int col = 0; col < cols; col++) {
            VTermScreenCell value = {0};
            if (actual < 0) {
                if (col >= t->history_cols[history]) continue;
                value = ((VTermScreenCell *)t->history[history])[col];
            } else if (!vterm_screen_get_cell(t->screen, (VTermPos){ actual, col }, &value)) {
                continue;
            }
            int x = surface.x + 11 + col * 9, y = surface.y + 10 + row * 21;
            uint32_t ink = terminal_color(t, value.fg, true, s->light);
            uint32_t base = terminal_color(t, value.bg, false, s->light);
            bool cursor = scroll == 0 && t->fd >= 0 && t->cursor_visible &&
                          row == t->cursor_row && col == t->cursor_col;
            if (value.attrs.reverse) {
                uint32_t swap = ink; ink = base; base = swap;
            }
            if (!VTERM_COLOR_IS_DEFAULT_BG(&value.bg) || value.attrs.reverse)
                box(s->d, rect(x, y, 9, 21), base);
            if (cursor) box(s->d, rect(x, y + 2, 2, 16), s->light ? 0x5347cd : 0xc6bbff);
            if (!value.chars[0] || value.chars[0] == UINT32_MAX) continue;
            uint32_t ch = value.chars[0];
            if (ch == 0x2500 || ch == 0x2502) {
                if (ch == 0x2500) line(s->d, x, y + 10, x + 8, y + 10, ink);
                else line(s->d, x + 4, y, x + 4, y + 20, ink);
            } else {
                char chr[] = { ch > 126 ? '?' : (char)ch, 0 };
                text(s->d, x, y, chr, FACE_MONO, ink, surface.x + surface.w - 5);
            }
            if (value.attrs.underline) box(s->d, rect(x, y + 17, 9, 1), ink);
        }
    }
}

bool terminal_contains(const Terminal *t, const char *needle)
{
    if (!t->screen) return false;
    char line_text[1024];
    for (int row = 0; row < t->rows; row++) {
        size_t length = vterm_screen_get_text(t->screen, line_text, sizeof(line_text) - 1,
            (VTermRect){ row, row + 1, 0, t->cols });
        line_text[length] = 0;
        if (strstr(line_text, needle)) return true;
    }
    return false;
}
