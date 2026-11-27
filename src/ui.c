#include "kos.h"

#include <ctype.h>
#include <fcntl.h>
#include <stdio.h>
#include <signal.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/utsname.h>
#include <unistd.h>

#define TOP 55
#define NAV 58
#define ROW 23

typedef struct { uint32_t ink, muted, card, dim, edge, accent, on_accent, nav; } Colors;
static const struct { App app; const char *name, *detail; uint32_t tint; } apps[] = {
    { NOTES, "Notes", "Write and keep plain text", 0xf6c871 },
    { TERMINAL, "Terminal", "Run system commands", 0x99e6bd },
    { FILES, "Files", "Browse saved documents", 0xa8baff },
    { CALCULATOR, "Calculator", "Work out an expression", 0xf0a7c4 },
    { CLOCK, "Clock", "Local time and stopwatch", 0x8cdbef },
    { SETTINGS, "Settings", "Appearance and device info", 0xc4b2fb },
};

static Colors colors(const State *s)
{
    if (s->light) return (Colors){0x171731, 0x474c65, 0xffffff, 0xe4e8f6, 0xcad0e4,
                                  0x5337ad, 0xffffff, 0xecedf9};
    return (Colors){0xf8f6ff, 0xb8b9d2, 0x252842, 0x1b1e35, 0x393d5a,
                    0xd8cbff, 0x201636, 0x191b30};
}

static int max(int a, int b) { return a > b ? a : b; }
static int min(int a, int b) { return a < b ? a : b; }
static int margin(const State *s) { return s->d->w < 500 ? 16 : 28; }
static int rel_width(const State *s) { return s->d->w - margin(s) * 2; }
static int key_height(const State *s) { return s->d->h < 380 ? 22 : s->d->h < 465 ? 28 : 38; }
static int bottom(const State *s) { return s->d->h - NAV - (s->keyboard ? key_height(s) * 5 + 6 : 0); }
static int columns(const State *s) { return s->d->w >= 940 ? 3 : s->d->w >= 540 ? 2 : 1; }

static const char *app_name(App app)
{
    for (size_t i = 0; i < ARRAY_LEN(apps); i++)
        if (apps[i].app == app) return apps[i].name;
    return "KreativeOS";
}

static void centered(Display *d, Rect r, const char *label, Face face, uint32_t ink)
{
    int h = face == FACE_DISPLAY ? 52 : face == FACE_STRONG ? 24 : 18;
    int x = max(r.x + 3, r.x + (r.w - text_width(label, face)) / 2);
    text(d, x, r.y + (r.h - h) / 2, label, face, ink, r.x + r.w - 3);
}

static void button(State *s, Rect r, const char *label, bool selected)
{
    Colors c = colors(s);
    round_box(s->d, r, 12, selected ? c.accent : c.card);
    centered(s->d, r, label, FACE_REGULAR, selected ? c.on_accent : c.ink);
}

static void stroke(Display *d, int x, int y, int u, int v, uint32_t color)
{
    line(d, x, y, u, v, color);
    line(d, x + 1, y, u + 1, v, color);
}

static void icon(Display *d, int x, int y, App app, uint32_t tint)
{
    uint32_t fg = 0x20213a;
    round_box(d, rect(x, y, 43, 43), 13, tint);
    x += 9; y += 9;
    switch (app) {
    case NOTES:
        round_box(d, rect(x + 3, y, 20, 24), 4, fg);
        for (int i = 6; i <= 17; i += 5) box(d, rect(x + 7, y + i, 11, 2), tint);
        break;
    case TERMINAL:
        stroke(d, x + 3, y + 5, x + 10, y + 12, fg);
        stroke(d, x + 10, y + 12, x + 3, y + 19, fg);
        box(d, rect(x + 13, y + 18, 9, 2), fg);
        break;
    case FILES:
        round_box(d, rect(x + 1, y + 6, 24, 15), 3, fg);
        round_box(d, rect(x + 2, y + 3, 11, 8), 2, fg);
        box(d, rect(x + 4, y + 10, 18, 1), tint);
        break;
    case CALCULATOR:
        round_box(d, rect(x + 3, y, 20, 25), 4, fg);
        box(d, rect(x + 7, y + 4, 12, 4), tint);
        for (int r = 0; r < 2; r++)
            for (int col = 0; col < 3; col++)
                box(d, rect(x + 7 + col * 4, y + 12 + r * 5, 3, 3), tint);
        break;
    case CLOCK:
        circle(d, x + 12, y + 12, 11, fg);
        circle(d, x + 12, y + 12, 8, tint);
        stroke(d, x + 12, y + 7, x + 12, y + 12, fg);
        stroke(d, x + 12, y + 12, x + 16, y + 15, fg);
        break;
    case SETTINGS:
        for (int i = 0; i < 3; i++) {
            box(d, rect(x + 2, y + 5 + i * 7, 21, 2), fg);
            circle(d, x + (i == 1 ? 16 : 8), y + 6 + i * 7, 3, fg);
        }
        break;
    default: break;
    }
}

static Rect app_card(const State *s, int index)
{
    int cols = columns(s);
    int w = (rel_width(s) - 10 * (cols - 1)) / cols;
    return rect(margin(s) + (index % cols) * (w + 10), TOP + 177 + (index / cols) * 76 - s->scroll, w, 67);
}

static void draw_home(State *s)
{
    Display *d = s->d;
    Colors c = colors(s);
    int x = margin(s), y = TOP + 10 - s->scroll, w = rel_width(s);
    Rect hero = rect(x, y, w, 127);
    round_box(d, hero, 22, s->light ? 0xddd5fc : 0x302759);
    int px = x + w - 52, py = y + 64;
    circle(d, px, py, 38, s->light ? 0xc6b4fc : 0x51417f);
    circle(d, px - 5, py - 2, 26, s->light ? 0x9780e4 : 0x7864bd);
    circle(d, px - 10, py - 6, 13, 0xded0ff);
    stroke(d, px - 35, py + 26, px + 27, py - 27, s->light ? 0x6046ab : 0xb8a3f9);
    circle(d, px + 17, py - 31, 3, 0xf6c871);
    text(d, x + 17, y + 18, "KreativeOS", FACE_STRONG, c.ink, px - 39);
    time_t stamp = time(NULL);
    struct tm local;
    char date[48], now[10];
    if (localtime_r(&stamp, &local)) {
        strftime(now, sizeof(now), "%H:%M", &local);
        strftime(date, sizeof(date), "%A, %d %B", &local);
        text(d, x + 16, y + 39, now, FACE_DISPLAY, c.ink, px - 32);
        text(d, x + 18, y + 94, date, FACE_REGULAR, c.muted, px - 35);
    }
    text(d, x + 2, y + 138, "Apps", FACE_STRONG, c.ink, x + w);
    for (size_t i = 0; i < ARRAY_LEN(apps); i++) {
        Rect r = app_card(s, (int)i);
        if (r.y + r.h < TOP || r.y >= bottom(s)) continue;
        round_box(d, r, 16, c.card);
        icon(d, r.x + 11, r.y + 12, apps[i].app, apps[i].tint);
        text(d, r.x + 65, r.y + 11, apps[i].name, FACE_STRONG, c.ink, r.x + r.w - 20);
        text(d, r.x + 65, r.y + 38, apps[i].detail, FACE_REGULAR, c.muted, r.x + r.w - 16);
        if (r.w > 230) {
            line(d, r.x + r.w - 19, r.y + 29, r.x + r.w - 14, r.y + 34, c.muted);
            line(d, r.x + r.w - 14, r.y + 34, r.x + r.w - 19, r.y + 39, c.muted);
        }
    }
}

static Rect top_button(const State *s, int index)
{
    int x = margin(s);
    return rect(x + rel_width(s) - (index + 1) * 83, 8, 78, 39);
}

static Rect note_card(const State *s, int index)
{
    return rect(margin(s), TOP + 8 + index * 55 - s->scroll, rel_width(s), 49);
}

static void draw_list(State *s)
{
    Colors c = colors(s);
    if (s->list_dirty) notes_list(s);
    for (int i = 0; i < s->name_count; i++) {
        Rect r = note_card(s, i);
        if (r.y + r.h <= TOP || r.y >= bottom(s)) continue;
        round_box(s->d, r, 11, c.card);
        text(s->d, r.x + 12, r.y + 8, s->names[i], FACE_STRONG, c.ink, r.x + r.w - 55);
        text(s->d, r.x + r.w - 40, r.y + 13, ".txt", FACE_REGULAR, c.muted, r.x + r.w - 4);
    }
    if (!s->name_count) {
        const char *empty = s->app == FILES ? "Saved notes appear here as text files." :
                                              "Tap New to write your first note.";
        if (access(s->data, W_OK) < 0) empty = "Attach writable storage to save notes.";
        text(s->d, margin(s), TOP + 20, empty, FACE_REGULAR, c.muted, s->d->w - margin(s));
    }
    if (*s->message) text(s->d, margin(s), bottom(s) - 23, s->message, FACE_REGULAR,
                          c.accent, s->d->w - margin(s));
}

static size_t next_line(const char *buffer, size_t length, size_t start, int cols)
{
    for (int col = 0; col < cols && start < length; col++)
        if (buffer[start++] == '\n') break;
    return start;
}

static int line_count(const char *buf, size_t length, int cols)
{
    int lines = 1;
    for (size_t i = 0; i < length;) {
        i = next_line(buf, length, i, cols);
        if (i < length || buf[i - 1] == '\n') lines++;
    }
    return lines;
}

static int cursor_line(const char *buf, size_t length, size_t cursor, int cols, size_t *start)
{
    *start = 0;
    int line = 0;
    while (*start < length) {
        size_t end = next_line(buf, length, *start, cols);
        if (cursor < end || (cursor == end && end == length && buf[end - 1] != '\n')) break;
        *start = end;
        line++;
    }
    return line;
}

static int text_cols(const State *s) { return min(720, max(1, (rel_width(s) - 24) / 9)); }
static int note_top(void) { return TOP + 67; }
static int terminal_top(void) { return TOP + 12; }
static int terminal_input(const State *s) { return bottom(s) - 47; }
static int note_rows(const State *s) { return max(1, (bottom(s) - note_top() - 22) / ROW); }
static int terminal_rows(const State *s) { return max(1, (terminal_input(s) - terminal_top() - 2) / ROW); }

static void draw_buffer(State *s, const char *buf, size_t length, size_t cursor, int y, int rows,
                        int first, uint32_t color)
{
    int cols = text_cols(s), x = margin(s) + 12;
    char content[722];
    size_t offset = 0, start = 0;
    int caret = cursor <= length ? cursor_line(buf, length, cursor, cols, &start) : -1;
    for (int i = 0; i < first && offset < length; i++) offset = next_line(buf, length, offset, cols);
    for (int row = 0; row < rows && offset <= length; row++) {
        size_t end = next_line(buf, length, offset, cols);
        size_t count = end - offset;
        if (count && buf[end - 1] == '\n') count--;
        memcpy(content, buf + offset, count);
        content[count] = 0;
        text(s->d, x, y + row * ROW, content, FACE_MONO, color, s->d->w - margin(s) - 6);
        if (caret == first + row && start == offset)
            box(s->d, rect(x + (int)(cursor - start) * 9, y + row * ROW + 2, 2, 17), colors(s).accent);
        if (offset == end) break;
        offset = end;
    }
}

static Rect name_field(const State *s)
{
    return rect(margin(s), TOP + 9, rel_width(s), 43);
}

static void draw_note(State *s)
{
    if (!s->note.open) { draw_list(s); return; }
    Colors c = colors(s);
    Rect title = name_field(s);
    round_box(s->d, title, 12, s->note.editing_name ? c.edge : c.card);
    text(s->d, title.x + 12, title.y + 8, *s->note.name ? s->note.name : "Name your note",
         FACE_STRONG, *s->note.name ? c.ink : c.muted, title.x + title.w - 10);
    int body_height = bottom(s) - note_top() - 10;
    round_box(s->d, rect(margin(s), note_top() - 6, rel_width(s), body_height + 5), 12, c.card);
    draw_buffer(s, s->note.text, s->note.length, s->note.editing_name ? SIZE_MAX : s->note.cursor,
                note_top(), note_rows(s), s->scroll, c.ink);
    const char *status = *s->message ? s->message : s->note.dirty ? "Editing" :
                         s->note.editing_name ? "Enter a title to begin" : "Plain text";
    text(s->d, margin(s) + 10, bottom(s) - 22, status, FACE_REGULAR, c.accent, s->d->w - margin(s));
}

static void draw_terminal(State *s)
{
    Colors c = colors(s);
    int x = margin(s), width = rel_width(s), input = terminal_input(s);
    round_box(s->d, rect(x, TOP + 5, width, input - TOP - 8), 14, c.dim);
    int first = max(0, line_count(s->term.output, s->term.output_len, text_cols(s)) -
                       terminal_rows(s) - s->scroll);
    draw_buffer(s, s->term.output, s->term.output_len, SIZE_MAX, terminal_top(),
                terminal_rows(s), first, c.ink);
    round_box(s->d, rect(x, input, width, 40), 10, c.card);
    text(s->d, x + 10, input + 10, "$", FACE_MONO, c.accent, x + 30);
    int cols = max(1, (width - 36) / 9);
    size_t skip = s->term.cursor >= (size_t)cols ? s->term.cursor - cols + 1 : 0;
    text(s->d, x + 27, input + 10, s->term.line + skip, FACE_MONO, c.ink, x + width - 6);
    if (!s->term.pid)
        box(s->d, rect(x + 27 + (int)(s->term.cursor - skip) * 9, input + 11, 2, 17), c.accent);
}

static const char calc_keys[] = "789/456*123-0.()+<C=";
static Rect calc_button(const State *s, int i)
{
    int width = min(rel_width(s), 530), x = (s->d->w - width) / 2;
    int step = width / 4, height = min(63, (bottom(s) - TOP - 96) / 5);
    return rect(x + (i % 4) * step, TOP + 87 + (i / 4) * height, step - 5, height - 4);
}

static void draw_calculator(State *s)
{
    Colors c = colors(s);
    int width = min(rel_width(s), 530), x = (s->d->w - width) / 2;
    round_box(s->d, rect(x, TOP + 8, width - 5, 69), 13, c.dim);
    text(s->d, x + 12, TOP + 18, s->expression, FACE_MONO, c.ink, x + width - 12);
    text(s->d, x + 12, TOP + 38, *s->result ? s->result : "0", FACE_STRONG, c.accent, x + width - 12);
    for (size_t i = 0; i < sizeof(calc_keys) - 1; i++) {
        char label[] = { calc_keys[i], 0 };
        button(s, calc_button(s, (int)i), calc_keys[i] == '<' ? "Del" : label, calc_keys[i] == '=');
    }
}

static Rect clock_button(const State *s, int index)
{
    int width = min(rel_width(s), 400), x = (s->d->w - width) / 2;
    return rect(x + index * (width / 2), min(TOP + 177, bottom(s) - 47), width / 2 - 5, 42);
}

static void draw_clock(State *s)
{
    Colors c = colors(s);
    int x = margin(s), w = rel_width(s);
    time_t stamp = time(NULL);
    struct tm local;
    char value[24] = "";
    if (localtime_r(&stamp, &local)) strftime(value, sizeof(value), "%H:%M:%S", &local);
    centered(s->d, rect(x, TOP + 11, w, 68), value, FACE_DISPLAY, c.ink);
    centered(s->d, rect(x, TOP + 89, w, 23), "Stopwatch", FACE_REGULAR, c.muted);
    unsigned elapsed = s->stopwatch_saved;
    if (s->stopwatch_running) elapsed += s->now - s->stopwatch_start;
    snprintf(value, sizeof(value), "%02u:%02u.%u", elapsed / 60000, elapsed / 1000 % 60, elapsed / 100 % 10);
    centered(s->d, rect(x, TOP + 115, w, 43), value, FACE_STRONG, c.accent);
    button(s, clock_button(s, 0), s->stopwatch_running ? "Pause" : "Start", true);
    button(s, clock_button(s, 1), "Reset", false);
}

static Rect setting_button(const State *s, int index)
{
    return rect(margin(s), (index ? min(TOP + 177, bottom(s) - 47) : TOP + 14), min(rel_width(s), 290), 44);
}

static void draw_settings(State *s)
{
    Colors c = colors(s);
    int x = margin(s), right = s->d->w - x;
    button(s, setting_button(s, 0), s->light ? "Use dark appearance" : "Use light appearance", false);
    struct utsname platform;
    char description[110];
    if (uname(&platform) == 0) {
        snprintf(description, sizeof(description), "%.28s %.52s", platform.machine, platform.release);
        text(s->d, x, TOP + 73, description, FACE_REGULAR, c.ink, right);
    }
    snprintf(description, sizeof(description), "Display  %d x %d", s->d->device_w, s->d->device_h);
    text(s->d, x, TOP + 97, description, FACE_REGULAR, c.muted, right);
    snprintf(description, sizeof(description), "Storage  %s", access(s->data, W_OK) != 0 ? "Unavailable" :
             getenv("KOS_VOLATILE") ? "Temporary (RAM)" : "Writable");
    text(s->d, x, TOP + 121, description, FACE_REGULAR, c.muted, right);
    const char *power = getenv("KOS_APPLIANCE") ? "Shut down" : "End session";
    button(s, setting_button(s, 1), s->confirm_power ? "Tap again to confirm" : power, s->confirm_power);
}

typedef struct { Rect box; int key; const char *label; } SoftKey;
enum { SHIFT_UI = 400, SYMBOL_UI, HIDE_UI };

static int keyboard_layout(const State *s, SoftKey keys[52])
{
    static const char *plain[] = { "1234567890", "qwertyuiop", "asdfghjkl", "zxcvbnm" };
    static const char *symbols[] = { "!@#$%^&*()", "-_+=[]{}|\\", ";:'\",.<>?", "/`~" };
    int n = 0, w = s->d->w, h = key_height(s), y = bottom(s) + 4;
    for (int row = 0; row < 4; row++) {
        const char *chars = s->symbols ? symbols[row] : plain[row];
        int len = (int)strlen(chars), step = (w - 16) / 10, px = (w - len * step) / 2;
        if (row == 3) {
            keys[n++] = (SoftKey){rect(5, y + row * h, px - 7, h - 3), SHIFT_UI, "Shift"};
            keys[n++] = (SoftKey){rect(px + len * step, y + row * h, w - px - len * step - 5, h - 3), '\b', "Del"};
        }
        for (int col = 0; col < len; col++) {
            int key = s->shift && !s->symbols ? toupper((unsigned char)chars[col]) : chars[col];
            keys[n++] = (SoftKey){rect(px + col * step, y + row * h, step - 3, h - 3), key, NULL};
        }
    }
    static const int key[] = { SYMBOL_UI, KEY_LEFT_UI, ' ', KEY_RIGHT_UI, KEY_UP_UI, KEY_DOWN_UI, '\n', HIDE_UI };
    static const char *labels[] = { "Sym", "<", "Space", ">", "^", "v", "Enter", "Hide" };
    static const int parts[] = { 12, 8, 24, 8, 8, 8, 18, 14 };
    int part = 0;
    for (size_t i = 0; i < ARRAY_LEN(key); i++) {
        int left = 5 + (w - 10) * part / 100;
        part += parts[i];
        int right = 5 + (w - 10) * part / 100;
        keys[n++] = (SoftKey){rect(left, y + 4 * h, right - left - 3, h - 3), key[i], labels[i]};
    }
    return n;
}

static Rect nav_button(const State *s, int i)
{
    int width = min(s->d->w - 14, 440), step = width / 3;
    return rect((s->d->w - width) / 2 + i * step + 3, s->d->h - NAV + 8, step - 6, 42);
}

static void draw_chrome(State *s)
{
    Display *d = s->d;
    Colors c = colors(s);
    int x = margin(s);
    box(d, rect(0, 0, d->w, TOP), c.nav);
    box(d, rect(0, TOP - 1, d->w, 1), c.edge);
    if (s->app == HOME) {
        round_box(d, rect(x, 12, 31, 31), 9, c.accent);
        stroke(d, x + 10, 20, x + 10, 33, c.on_accent);
        stroke(d, x + 10, 28, x + 21, 19, c.on_accent);
        stroke(d, x + 12, 27, x + 21, 34, c.on_accent);
        text(d, x + 40, 15, "Home", FACE_STRONG, c.ink, d->w - x);
    } else {
        text(d, x, 15, app_name(s->app), FACE_STRONG, c.ink, d->w - x);
    }
    if (s->app == NOTES) {
        if (s->note.open) {
            button(s, top_button(s, 1), "Notes list", false);
            button(s, top_button(s, 0), s->note.editing_name ? "Start" : "Save", true);
        } else button(s, top_button(s, 0), "New", true);
    }
    if (s->app == FILES) button(s, top_button(s, 0), "Refresh", false);
    if (s->app == TERMINAL) {
        if (s->term.pid) button(s, top_button(s, 1), "Stop", false);
        button(s, top_button(s, 0), s->keyboard ? "Hide keys" : "Keyboard", true);
    }
    if (s->keyboard) {
        box(d, rect(0, bottom(s), d->w, d->h - NAV - bottom(s)), c.nav);
        SoftKey keys[52];
        int count = keyboard_layout(s, keys);
        for (int i = 0; i < count; i++) {
            SoftKey key = keys[i];
            char label[] = { (char)key.key, 0 };
            button(s, key.box, key.label ? key.label : label,
                   key.key == '\n' || (key.key == SHIFT_UI && s->shift) ||
                   (key.key == SYMBOL_UI && s->symbols));
        }
    }
    box(d, rect(0, d->h - NAV, d->w, NAV), c.nav);
    box(d, rect(0, d->h - NAV, d->w, 1), c.edge);
    button(s, nav_button(s, 0), "Home", s->app == HOME);
    button(s, nav_button(s, 1), "Notes", s->app == NOTES);
    button(s, nav_button(s, 2), "Terminal", s->app == TERMINAL);
    if (s->show_pointer) {
        circle(d, s->mx, s->my, 5, c.nav);
        circle(d, s->mx, s->my, 3, c.accent);
    }
}

void ui_draw(State *s)
{
    wallpaper(s->d, s->light);
    switch (s->app) {
    case HOME: draw_home(s); break;
    case NOTES: draw_note(s); break;
    case TERMINAL: draw_terminal(s); break;
    case FILES: draw_list(s); break;
    case CALCULATOR: draw_calculator(s); break;
    case CLOCK: draw_clock(s); break;
    case SETTINGS: draw_settings(s); break;
    }
    draw_chrome(s);
}

static unsigned milliseconds(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (unsigned)ts.tv_sec * 1000u + (unsigned)(ts.tv_nsec / 1000000);
}

static void go(State *s, App app)
{
    if (s->app == app) return;
    if (s->app == NOTES && !notes_save(s)) return;
    s->app = app;
    s->scroll = 0;
    s->keyboard = s->shift = s->symbols = s->confirm_power = false;
    s->message[0] = 0;
    s->list_dirty = true;
}

static int scroll_limit(State *s)
{
    switch (s->app) {
    case HOME: {
        Rect tail = app_card(s, (int)ARRAY_LEN(apps) - 1);
        return max(0, tail.y + tail.h + s->scroll + 12 - bottom(s));
    }
    case NOTES:
        if (s->note.open)
            return max(0, line_count(s->note.text, s->note.length, text_cols(s)) - note_rows(s));
        /* fall through */
    case FILES:
        return max(0, TOP + 8 + s->name_count * 55 - bottom(s));
    case TERMINAL:
        return max(0, line_count(s->term.output, s->term.output_len, text_cols(s)) - terminal_rows(s));
    default: return 0;
    }
}

static void reveal_cursor(State *s)
{
    size_t start;
    int row = cursor_line(s->note.text, s->note.length, s->note.cursor, text_cols(s), &start);
    if (row < s->scroll) s->scroll = row;
    if (row >= s->scroll + note_rows(s)) s->scroll = row - note_rows(s) + 1;
    s->scroll = min(s->scroll, scroll_limit(s));
}

static void begin_note(State *s)
{
    if (!s->note.editing_name) return;
    if (!*s->note.name) {
        strcpy(s->message, "Enter a title first");
        return;
    }
    notes_list(s);
    for (int i = 0; i < s->name_count; i++) {
        if (strcasecmp(s->names[i], s->note.name) == 0) {
            strcpy(s->message, "A note already has that name");
            return;
        }
    }
    size_t n = strlen(s->note.name);
    if (s->note.name[n - 1] == ' ') {
        strcpy(s->message, "Remove the trailing space in the title");
        return;
    }
    s->note.editing_name = false;
    s->note.dirty = true;
    s->message[0] = 0;
    s->note.next_save = s->now + 750;
}

static void note_key(State *s, int key)
{
    Note *note = &s->note;
    if (!note->open) {
        if (key == '\n') notes_new(s);
        return;
    }
    if (note->editing_name) {
        size_t len = strlen(note->name);
        if (key == '\n') begin_note(s);
        else if (key == '\b' && len) note->name[len - 1] = 0;
        else if (key >= 32 && key <= 126 && len < sizeof(note->name) - 1 &&
                 (isalnum(key) || (key == ' ' && len) || key == '-' || key == '_')) {
            note->name[len] = (char)key;
            note->name[len + 1] = 0;
            s->message[0] = 0;
        }
        return;
    }
    bool changed = false;
    if (key == KEY_LEFT_UI && note->cursor) note->cursor--;
    else if (key == KEY_RIGHT_UI && note->cursor < note->length) note->cursor++;
    else if (key == KEY_UP_UI || key == KEY_DOWN_UI) {
        int cols = text_cols(s);
        size_t start;
        int row = cursor_line(note->text, note->length, note->cursor, cols, &start);
        int target = row + (key == KEY_UP_UI ? -1 : 1);
        if (target >= 0 && target < line_count(note->text, note->length, cols)) {
            size_t next = 0;
            for (int i = 0; i < target; i++) next = next_line(note->text, note->length, next, cols);
            size_t end = next_line(note->text, note->length, next, cols);
            if (end > next && (end < note->length || note->text[end - 1] == '\n')) end--;
            size_t position = next + note->cursor - start;
            note->cursor = position < end ? position : end;
        }
    } else if ((key == '\b' && note->cursor) || (key == KEY_DELETE_UI && note->cursor < note->length)) {
        if (key == KEY_DELETE_UI) note->cursor++;
        memmove(note->text + note->cursor - 1, note->text + note->cursor, note->length - note->cursor + 1);
        note->cursor--;
        note->length--;
        changed = true;
    } else if (((key >= 32 && key <= 126) || key == '\n') && note->length < NOTE_CAP) {
        memmove(note->text + note->cursor + 1, note->text + note->cursor, note->length - note->cursor + 1);
        note->text[note->cursor++] = (char)key;
        note->length++;
        changed = true;
    }
    if (changed) {
        note->dirty = true;
        note->next_save = s->now + 750;
        s->message[0] = 0;
    }
    reveal_cursor(s);
}

static void calc_key(State *s, int key)
{
    size_t n = strlen(s->expression);
    if (key == 'C') {
        s->expression[0] = s->result[0] = 0;
    } else if ((key == '\b' || key == '<') && n) {
        s->expression[n - 1] = 0;
    } else if (key == '\n' || key == '=') {
        double result;
        if (calculate(s->expression, &result)) snprintf(s->result, sizeof(s->result), "%.12g", result);
        else strcpy(s->result, "Invalid expression");
    } else if (key >= 32 && key <= 126 && strchr("0123456789.()+-*/ ", key) &&
               n < sizeof(s->expression) - 1) {
        s->expression[n] = (char)key;
        s->expression[n + 1] = 0;
    }
}

void ui_key(State *s, int key)
{
    if (key == NO_KEY) return;
    if (key == KEY_HOME_UI) go(s, HOME);
    else if (key == KEY_SAVE_UI) notes_save(s);
    else {
        switch (s->app) {
        case NOTES: note_key(s, key); break;
        case TERMINAL: terminal_key(s, key); break;
        case CALCULATOR: calc_key(s, key); break;
        case HOME: case FILES:
            if (key == KEY_UP_UI || key == KEY_DOWN_UI) ui_scroll(s, key == KEY_UP_UI ? -65 : 65);
            break;
        default: break;
        }
    }
    s->redraw = true;
}

static bool hit(const State *s, Rect bounds)
{
    return inside(bounds, s->mx, s->my);
}

static void change_appearance(State *s)
{
    s->light = !s->light;
    char path[PATH_MAX];
    int length = snprintf(path, sizeof(path), "%s/.appearance", s->data);
    if (length < 0 || (size_t)length >= sizeof(path)) return;
    if (!*s->data) return;
    int fd = open(path, O_WRONLY | O_CREAT | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK, 0644);
    if (fd < 0) return;
    struct stat st;
    if (fstat(fd, &st) != 0 || !S_ISREG(st.st_mode) || ftruncate(fd, 0) != 0) {
        close(fd);
        return;
    }
    FILE *file = fdopen(fd, "w");
    if (file) {
        fputs(s->light ? "light\n" : "dark\n", file);
        fclose(file);
    } else close(fd);
}

static void click(State *s)
{
    for (int i = 0; i < 3; i++) {
        static const App nav[] = { HOME, NOTES, TERMINAL };
        if (hit(s, nav_button(s, i))) { go(s, nav[i]); return; }
    }
    if (s->keyboard && s->my >= bottom(s)) {
        SoftKey keys[52];
        int n = keyboard_layout(s, keys);
        for (int i = 0; i < n; i++) {
            if (!hit(s, keys[i].box)) continue;
            int key = keys[i].key;
            if (key == SHIFT_UI) s->shift = !s->shift;
            else if (key == SYMBOL_UI) s->symbols = !s->symbols;
            else if (key == HIDE_UI) {
                s->keyboard = false;
                s->scroll = min(s->scroll, scroll_limit(s));
            } else {
                ui_key(s, key);
                s->shift = false;
            }
            return;
        }
        return;
    }
    if (s->my < TOP) {
        if (s->app == NOTES) {
            if (hit(s, top_button(s, 0))) {
                if (!s->note.open) notes_new(s);
                else if (s->note.editing_name) begin_note(s);
                else notes_save(s);
            } else if (s->note.open && hit(s, top_button(s, 1)) && notes_save(s)) {
                s->note.open = false;
                s->keyboard = false;
                s->scroll = 0;
                s->message[0] = 0;
                s->list_dirty = true;
            }
        } else if (s->app == FILES && hit(s, top_button(s, 0))) {
            s->list_dirty = true;
            s->scroll = 0;
        } else if (s->app == TERMINAL) {
            if (hit(s, top_button(s, 0))) {
                s->keyboard = !s->keyboard;
                s->scroll = 0;
            }
            if (s->term.pid && hit(s, top_button(s, 1))) terminal_key(s, KEY_BREAK_UI);
        }
        return;
    }
    if (s->my >= bottom(s)) return;
    switch (s->app) {
    case HOME:
        for (size_t i = 0; i < ARRAY_LEN(apps); i++)
            if (hit(s, app_card(s, (int)i))) { go(s, apps[i].app); break; }
        break;
    case NOTES:
        if (s->note.open) {
            if (s->note.editing_name) {
                if (hit(s, name_field(s))) s->keyboard = true;
            } else if (s->my >= note_top() - 6) {
                int row = s->scroll + max(0, (s->my - note_top()) / ROW);
                int cols = text_cols(s);
                size_t offset = 0;
                for (int i = 0; i < row && offset < s->note.length; i++)
                    offset = next_line(s->note.text, s->note.length, offset, cols);
                size_t end = next_line(s->note.text, s->note.length, offset, cols);
                if (end > offset && (end < s->note.length || s->note.text[end - 1] == '\n')) end--;
                size_t selected = offset + max(0, (s->mx - margin(s) - 8) / 9);
                s->note.cursor = selected < end ? selected : end;
                s->keyboard = true;
                reveal_cursor(s);
            }
            break;
        }
        /* fall through */
    case FILES:
        for (int i = 0; i < s->name_count; i++)
            if (hit(s, note_card(s, i))) { notes_open(s, s->names[i]); break; }
        break;
    case TERMINAL:
        if (s->my >= terminal_input(s)) { s->keyboard = true; s->scroll = 0; }
        break;
    case CALCULATOR:
        for (size_t i = 0; i < sizeof(calc_keys) - 1; i++)
            if (hit(s, calc_button(s, (int)i))) { calc_key(s, calc_keys[i]); break; }
        break;
    case CLOCK:
        if (hit(s, clock_button(s, 0))) {
            if (s->stopwatch_running) s->stopwatch_saved += s->now - s->stopwatch_start;
            else s->stopwatch_start = s->now;
            s->stopwatch_running = !s->stopwatch_running;
        } else if (hit(s, clock_button(s, 1))) {
            s->stopwatch_saved = 0;
            s->stopwatch_start = s->now;
        }
        break;
    case SETTINGS:
        if (hit(s, setting_button(s, 0))) change_appearance(s);
        else if (hit(s, setting_button(s, 1)) && notes_save(s)) {
            if (s->confirm_power) s->running = false;
            s->confirm_power = true;
        } else s->confirm_power = false;
        break;
    }
}

void ui_scroll(State *s, int delta)
{
    int units = delta;
    if (s->app == NOTES && s->note.open) units = delta / ROW;
    if (s->app == TERMINAL) units = -delta / ROW;
    if (units == 0 && delta) units = (delta > 0 ? 1 : -1) * (s->app == TERMINAL ? -1 : 1);
    s->scroll = max(0, min(s->scroll + units, scroll_limit(s)));
    s->redraw = true;
}

void ui_pointer(State *s, int x, int y, bool down, bool show)
{
    s->mx = max(0, min(s->d->w - 1, x));
    s->my = max(0, min(s->d->h - 1, y));
    s->show_pointer = show;
    if (down && !s->pointer) {
        s->start_x = s->mx;
        s->start_y = s->my;
        s->start_scroll = s->scroll;
        s->dragged = false;
    }
    if (s->pointer) {
        int dx = s->mx - s->start_x, dy = s->my - s->start_y;
        if (abs(dx) > 7 || abs(dy) > 7) s->dragged = true;
        if (s->dragged && s->start_y >= TOP && s->start_y < bottom(s)) {
            s->scroll = s->start_scroll;
            ui_scroll(s, -dy);
        } else if (!down && !s->dragged) {
            click(s);
        }
    }
    s->pointer = down;
    s->redraw = true;
}

void ui_init(State *s, Display *d, const char *data)
{
    s->d = d;
    s->running = s->redraw = s->list_dirty = true;
    s->now = milliseconds();
    s->mx = d->w / 2;
    s->my = d->h / 2;
    s->term.fd = -1;
    static const char welcome[] = "KreativeOS Terminal\nTry uname -m, ls, or date.\n";
    memcpy(s->term.output, welcome, sizeof(welcome));
    s->term.output_len = sizeof(welcome) - 1;
    if (data && strlen(data) < sizeof(s->data) - 14) {
        strcpy(s->data, data);
        strcpy(s->term.cwd, data);
        char path[PATH_MAX];
        size_t len = strlen(data);
        memcpy(path, data, len);
        memcpy(path + len, "/.appearance", sizeof("/.appearance"));
        int fd = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK);
        struct stat st;
        FILE *file = fd >= 0 && fstat(fd, &st) == 0 && S_ISREG(st.st_mode) ? fdopen(fd, "r") : NULL;
        if (file) {
            char choice[8];
            s->light = fgets(choice, sizeof(choice), file) && !strcmp(choice, "light\n");
            fclose(file);
        } else if (fd >= 0) close(fd);
    }
    if (!*s->term.cwd) strcpy(s->term.cwd, "/");
}

void ui_tick(State *s)
{
    unsigned now = milliseconds(), before = s->now;
    s->now = now;
    if (s->app == CLOCK && before / 100 != now / 100) s->redraw = true;
    if (s->app == HOME && before / 1000 != now / 1000) s->redraw = true;
    if (s->note.dirty && (int)(now - s->note.next_save) >= 0) {
        notes_save(s);
        s->note.next_save = now + 2500;
        if (s->app == NOTES) s->redraw = true;
    }
    terminal_poll(s);
}

static void tap(State *s, Rect r)
{
    ui_pointer(s, r.x + r.w / 2, r.y + r.h / 2, true, false);
    ui_pointer(s, r.x + r.w / 2, r.y + r.h / 2, false, false);
}

static void type(State *s, const char *str)
{
    for (; *str; str++) ui_key(s, *str);
}

int ui_test(void)
{
    static State s;
    Display d;
    char test_dir[] = "/tmp/kreative-test-XXXXXX";
    if (!mkdtemp(test_dir) || !display_memory(&d, 360, 640)) return 1;
    if (geteuid() == 0 && chown(test_dir, 65534, 65534) < 0) return 1;
    ui_init(&s, &d, test_dir);
    double number;
    const char *failure = "calculator";
    if (!(calculate("-(2+3)*4.5/2", &number) && number == -11.25 &&
        !calculate("1/0", &number) && !calculate("2+)", &number))) goto fail;
    failure = "home swipe and Settings tile";
    ui_pointer(&s, 160, 492, true, false);
    ui_pointer(&s, 160, 142, true, false);
    ui_pointer(&s, 160, 142, false, false);
    if (s.scroll <= 0 || s.app != HOME) goto fail;
    tap(&s, app_card(&s, 5));
    if (s.app != SETTINGS) goto fail;
    failure = "touch keyboard and note persistence";
    tap(&s, nav_button(&s, 1));
    tap(&s, top_button(&s, 0));
    SoftKey keys[52];
    int count = keyboard_layout(&s, keys);
    for (int i = 0; i < count; i++) if (keys[i].key == 'n') tap(&s, keys[i].box);
    if (strcmp(s.note.name, "n") != 0) goto fail;
    type(&s, "ote\nKreativeOS\nWrite?");
    ui_key(&s, '\b');
    ui_key(&s, '!');
    if (!notes_save(&s)) goto fail;
    tap(&s, top_button(&s, 1));
    ui_draw(&s);
    tap(&s, note_card(&s, 0));
    if (!s.note.open || strcmp(s.note.text, "KreativeOS\nWrite!") != 0) goto fail;
    failure = "terminal pipeline and history";
    tap(&s, nav_button(&s, 2));
    type(&s, "printf 'shell works\\n' | cat\n");
    for (int i = 0; i < 100 && s.term.pid; i++) { usleep(10000); ui_tick(&s); }
    if (s.term.pid || !strstr(s.term.output, "\nshell works\n")) goto fail;
    ui_key(&s, KEY_UP_UI);
    if (strcmp(s.term.line, "printf 'shell works\\n' | cat")) goto fail;
    ui_key(&s, KEY_DOWN_UI);
    if (*s.term.line) goto fail;
    failure = NULL;
fail:
    if (s.term.pid > 0) kill(-s.term.pid, 9);
    char file[256];
    snprintf(file, sizeof(file), "%s/note.txt", test_dir);
    unlink(file);
    rmdir(test_dir);
    display_close(&d);
    if (failure) {
        fprintf(stderr, "FAIL: %s\n", failure);
        return 1;
    }
    puts("PASS: touch navigation, keyboard, saved notes, calculator and shell");
    return 0;
}
