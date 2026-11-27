#include "kos.h"

#include <ctype.h>
#include <fcntl.h>
#include <stdio.h>
#include <signal.h>
#include <math.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/utsname.h>
#include <unistd.h>

#define TOP 55
#define NAV 58
#define ROW 23

typedef struct { uint32_t ink, muted, card, dim, edge, accent, on_accent, nav; } Colors;
static const struct { App app; const char *name; uint32_t tint; } apps[] = {
    { NOTES, "Notes", 0xffb347 },
    { TERMINAL, "Terminal", 0x69d2bb },
    { FILES, "Files", 0x88b4ef },
    { CALCULATOR, "Calculator", 0xfa91ab },
    { CLOCK, "Clock", 0x7bc6e4 },
    { NETWORK, "Network", 0xaa98f0 },
    { SETTINGS, "Settings", 0xb3bac9 },
};

static Colors colors(const State *s)
{
    if (s->light) return (Colors){0x1b2034, 0x575e73, 0xffffff, 0xeff0f7, 0xe0e2ee,
                                  0x5241c8, 0xffffff, 0xf8f8fc};
    return (Colors){0xf8f6ff, 0xb8b9d2, 0x252842, 0x1b1e35, 0x393d5a,
                    0xd8cbff, 0x201636, 0x191b30};
}

static int max(int a, int b) { return a > b ? a : b; }
static int min(int a, int b) { return a < b ? a : b; }
static int margin(const State *s) { return s->d->w < 500 ? 16 : 28; }
static int rel_width(const State *s) { return s->d->w - margin(s) * 2; }
static int key_height(const State *s) { return s->d->h < 380 ? 22 : s->d->h < 465 ? 28 : 38; }
static int bottom(const State *s) { return s->d->h - NAV - (s->keyboard ? key_height(s) * 5 + 6 : 0); }
static int columns(const State *s) { return s->d->w >= 610 || s->d->h < 475 ? 4 : 2; }
static int home_inset(const State *s) { return max(margin(s), (s->d->w - 880) / 2); }
static int home_width(const State *s) { return s->d->w - home_inset(s) * 2; }
static int hero_height(const State *s) { return s->d->h < 475 ? 84 : s->d->h < 550 ? 108 : s->d->h < 610 ? 150 : 194; }
#define PAGE_SIZE 4
#define PAGE_COUNT ((APP_COUNT + PAGE_SIZE - 1) / PAGE_SIZE)

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
    case NETWORK:
        for (int i = 0; i < 3; i++) {
            int yoff = 6 + i * 5, width = 10 - i * 3;
            stroke(d, x + 12 - width, y + yoff, x + 12, y + yoff - 3, fg);
            stroke(d, x + 12, y + yoff - 3, x + 12 + width, y + yoff, fg);
        }
        circle(d, x + 12, y + 19, 2, fg);
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
    int cols = columns(s), pos = index % PAGE_SIZE, page = index / PAGE_SIZE;
    int gap = 10, width = home_width(s), w = (width - gap * (cols - 1)) / cols;
    int y = TOP + 27 + hero_height(s) + (pos / cols) * 106;
    return rect(home_inset(s) + (pos % cols) * (w + gap) +
                (int)lround((page - s->page_position) * s->d->w), y, w, 96);
}

static Rect page_dot(const State *s, int page)
{
    int y = TOP + 29 + hero_height(s) + (PAGE_SIZE / columns(s)) * 106;
    return rect(s->d->w / 2 - 48 + page * 48, y, 48, 41);
}

static void draw_home(State *s)
{
    Display *d = s->d;
    Colors c = colors(s);
    int x = home_inset(s), y = TOP + 12, w = home_width(s), h = hero_height(s);
    bool compact = h <= 108;
    time_t stamp = time(NULL);
    struct tm local;
    char date[48], now[10];
    if (localtime_r(&stamp, &local)) {
        strftime(now, sizeof(now), "%H:%M", &local);
        strftime(date, sizeof(date), "%A  %d %B", &local);
        const char *greet = local.tm_hour < 12 ? "Good morning" :
                            local.tm_hour < 18 ? "Good afternoon" : "Good evening";
        text(d, x + 5, y + (compact ? 2 : 12), greet, FACE_REGULAR, 0xe5e9f4, x + w);
        text(d, x + 2, y + (compact ? 20 : 34), now, FACE_DISPLAY, 0xffffff, x + w);
        if (!compact) text(d, x + 5, y + 93, date, FACE_REGULAR, 0xf1f2f8, x + w);
    }
    if (s->drawer || s->tasks) {
        round_box(d, rect(8, TOP + 4, d->w - 16, bottom(s) - TOP - 8), 24, c.nav);
        text(d, 25, TOP + 19, s->tasks ? "Recent apps" : "All apps", FACE_STRONG, c.ink, d->w - 25);
        int count = s->tasks ? s->recent_count : APP_COUNT;
        int cols = columns(s);
        int step = (d->w - 42) / cols;
        for (int i = 0; i < count; i++) {
            int idx = s->tasks ? 0 : s->order[i];
            if (s->tasks) for (int j = 0; j < APP_COUNT; j++) if (apps[j].app == s->recent[i]) idx = j;
            Rect tile = rect(21 + i % cols * step, TOP + 57 + i / cols * 96, step - 5, 91);
            icon(d, tile.x + (tile.w - 43) / 2, tile.y + 8, apps[idx].app, apps[idx].tint);
            centered(d, rect(tile.x, tile.y + 54, tile.w, 30), apps[idx].name, FACE_REGULAR, c.ink);
        }
        return;
    }
    for (int i = 0; i < APP_COUNT; i++) {
        Rect tile = app_card(s, i);
        if (tile.x + tile.w <= 0 || tile.x >= d->w || tile.y >= bottom(s)) continue;
        int index = s->order[i];
        bool selected = i == s->selection;
        if (selected) round_box(d, tile, 19, c.accent);
        icon(d, tile.x + (tile.w - 43) / 2, tile.y + 12, apps[index].app, apps[index].tint);
        centered(d, rect(tile.x, tile.y + 57, tile.w, 30), apps[index].name, FACE_REGULAR, 0xffffff);
    }
    if (page_dot(s, 0).y + 24 < bottom(s)) {
        for (int p = 0; p < PAGE_COUNT; p++) {
            Rect hit = page_dot(s, p);
            circle(d, hit.x + hit.w / 2, hit.y + 14, p == s->page ? 5 : 4, p == s->page ? c.accent : c.edge);
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
static int note_rows(const State *s) { return max(1, (bottom(s) - note_top() - 22) / ROW); }

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

static Rect terminal_area(const State *s)
{
    return rect(margin(s), TOP + 9, rel_width(s), bottom(s) - TOP - 17);
}

static void draw_terminal(State *s)
{
    Rect r = terminal_area(s);
    round_box(s->d, r, 15, s->light ? 0xffffff : 0x1a1e2f);
    terminal_draw(s, r, s->scroll);
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
    int w = min(rel_width(s), 620), x = (s->d->w - w) / 2;
    int y = TOP + 12 - s->scroll;
    if (index > 0) y += 62;
    if (index > 1) y += 54;
    if (index > 2) y += 54;
    return rect(x, y, w, 47);
}

static void draw_settings(State *s)
{
    Colors c = colors(s);
    button(s, setting_button(s, 0), s->light ? "Use dark appearance" : "Use light appearance", false);
    button(s, setting_button(s, 1), "Network", false);
    int end_index = getenv("KOS_APPLIANCE") ? 3 : 2;
    if (getenv("KOS_APPLIANCE")) button(s, setting_button(s, 2), "Sign out", false);
    button(s, setting_button(s, end_index), s->confirm_power ? "Confirm" :
           getenv("KOS_APPLIANCE") ? "Shut down" : "End session", s->confirm_power);
    struct utsname platform;
    char description[160];
    int w = min(rel_width(s), 620), x = (s->d->w - w) / 2;
    int y = setting_button(s, end_index).y + 62;
    if (uname(&platform) == 0) {
        snprintf(description, sizeof(description), "%.26s  /  %d x %d", platform.machine,
                 s->d->device_w, s->d->device_h);
        text(s->d, x, y, description, FACE_REGULAR, c.muted, x + w);
    }
    snprintf(description, sizeof(description), "Signed in as %.32s%s", s->user,
             getenv("KOS_VOLATILE") ? "  /  Temporary storage" : "");
    text(s->d, x, y + 24, description, FACE_REGULAR, c.muted, x + w);
}

static int auth_x(const State *s) { return (s->d->w - min(rel_width(s), 440)) / 2; }
static int auth_width(const State *s) { return s->d->w - auth_x(s) * 2; }
static bool auth_compact(const State *s) { return s->d->h < 505 && s->keyboard; }
static int auth_fields(const State *s) { return s->login.add ? 4 : s->login.setup ? 3 : 2; }

static int auth_top(const State *s)
{
    int needed = 120 + auth_fields(s) * 51;
    return TOP + max(0, (bottom(s) - TOP - needed) / 3);
}

static Rect auth_field(const State *s, int idx)
{
    int x = auth_x(s), w = auth_width(s);
    bool small = auth_compact(s);
    return small ? rect(x + (idx % 2) * (w / 2 + 3), TOP + 7 + (idx / 2) * 44, w / 2 - 4, 40) :
        rect(x, auth_top(s) + 78 + idx * 51, w, 46);
}

static Rect auth_toggle(const State *s)
{
    return rect(auth_x(s), s->d->h - NAV + 7, min(115, auth_width(s) / 2 - 4), 44);
}

static Rect auth_action(const State *s)
{
    int w = min(178, auth_width(s) / 2);
    return rect(auth_x(s) + auth_width(s) - w, s->d->h - NAV + 7, w, 44);
}

static void masked(char *dest, size_t cap, const char *value)
{
    size_t count = strlen(value);
    if (count >= cap) count = cap - 1;
    memset(dest, '*', count);
    dest[count] = 0;
}

static void draw_auth(State *s)
{
    Colors c = colors(s);
    const Login *l = &s->login;
    bool small = auth_compact(s);
    int x = auth_x(s), w = auth_width(s);
    if (!small) {
        const char *title = l->setup ? "Create your account" : l->add ? "Add an account" : "Welcome back";
        text(s->d, x, auth_top(s) + 13, title, FACE_STRONG, c.ink, x + w);
        const char *detail = l->setup ? "Choose the owner login for this device." :
                             l->add ? "The device owner authorizes new accounts." : "Sign in to open your workspace.";
        text(s->d, x, auth_top(s) + 42, detail, FACE_REGULAR, c.muted, x + w);
    }
    const char *fields[] = {l->user, l->password, l->confirmation, l->owner_password};
    const char *labels[] = {"Username", "Password", "Confirm password", "Owner password"};
    for (int i = 0; i < auth_fields(s); i++) {
        Rect r = auth_field(s, i);
        round_box(s->d, r, 11, i == l->field ? c.accent : c.edge);
        round_box(s->d, rect(r.x + 1, r.y + 1, r.w - 2, r.h - 2), 10, c.card);
        char stars[98];
        masked(stars, sizeof(stars), fields[i]);
        const char *value = (l->show || i == 0) ? fields[i] : stars;
        const char *visible = *value ? value : labels[i];
        if (*value) {
            int limit = max(8, (r.w - 24) / 9);
            int len = (int)strlen(visible);
            if (len > limit) visible += len - limit;
        }
        text(s->d, r.x + 11, r.y + (small ? 10 : 13), visible, FACE_REGULAR,
             *value ? c.ink : c.muted, r.x + r.w - 10);
    }
    int status_y = min(bottom(s) - 21, auth_field(s, auth_fields(s) - 1).y + 49);
    if (*s->message && status_y > TOP) text(s->d, x, status_y, s->message, FACE_REGULAR, c.accent, x + w);
    else if ((l->setup || l->add) && status_y > TOP)
        text(s->d, x, status_y, "Use at least 8 characters for your password.", FACE_REGULAR, c.muted, x + w);
}

static Rect network_row(const State *s, int index)
{
    return rect(margin(s), TOP + 80 + index * 49 - s->scroll, rel_width(s), 45);
}

static void draw_network(State *s)
{
    Colors c = colors(s);
    int x = margin(s), w = rel_width(s);
    text(s->d, x, TOP + 16, s->network_status, FACE_STRONG, c.ink, x + w);
    if (s->wifi_edit) {
        text(s->d, x, TOP + 47, s->wifi_ssid, FACE_REGULAR, c.muted, x + w);
        char password[65];
        masked(password, sizeof(password), s->wifi_password);
        Rect r = network_row(s, 0);
        round_box(s->d, r, 10, c.card);
        text(s->d, r.x + 13, r.y + 12, *password ? password : "Wi-Fi password", FACE_REGULAR, c.ink, x + w - 6);
    } else {
        for (int i = 0; i < s->wifi_count; i++) {
            Rect r = network_row(s, i);
            if (r.y < TOP + 70 || r.y + r.h > bottom(s) - 23) continue;
            round_box(s->d, r, 10, c.card);
            text(s->d, r.x + 13, r.y + 12, s->wifi_results[i], FACE_REGULAR, c.ink, x + w - 75);
            const char *mode = s->wifi_secured[i] ? "Secured" : "Open";
            text(s->d, r.x + r.w - 68, r.y + 12, mode, FACE_REGULAR, c.muted, x + w - 6);
        }
    }
    if (*s->message) text(s->d, x, min(TOP + 50, bottom(s) - 20),
                          s->message, FACE_REGULAR, c.muted, x + w);
}

typedef struct { Rect box; int key; const char *label; } SoftKey;
enum { SHIFT_UI = 400, SYMBOL_UI, HIDE_UI, CTRL_UI };

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
        bool control = key[i] == HIDE_UI && s->app == TERMINAL;
        keys[n++] = (SoftKey){rect(left, y + 4 * h, right - left - 3, h - 3),
                               control ? CTRL_UI : key[i], control ? "Ctrl" : labels[i]};
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
    bool home = s->app == HOME && !s->drawer && !s->tasks;
    if (!home) {
        box(d, rect(0, 0, d->w, TOP), c.nav);
        box(d, rect(0, TOP - 1, d->w, 1), c.edge);
    }
    if (s->app == AUTH) {
        round_box(d, rect(x, 11, 32, 32), 11, c.accent);
        stroke(d, x + 10, 20, x + 10, 33, c.on_accent);
        stroke(d, x + 10, 28, x + 21, 19, c.on_accent);
        stroke(d, x + 12, 27, x + 21, 34, c.on_accent);
        text(d, x + 42, 15, "KreativeOS", FACE_STRONG, c.ink, d->w - x);
    } else if (s->app == HOME) {
        text(d, x, 15, "KreativeOS", FACE_STRONG, 0xffffff, d->w - x);
    } else {
        text(d, x, 15, app_name(s->app), FACE_STRONG, c.ink, d->w - x);
    }
    if (s->app == HOME) {
        Rect edit = top_button(s, 0);
        round_box(d, edit, 16, s->arranging ? c.accent : 0x263951);
        centered(d, edit, s->arranging ? "Done" : "Edit", FACE_REGULAR, 0xffffff);
    }
    if (s->app == NOTES) {
        if (s->note.open) {
            button(s, top_button(s, 2), s->delete_note ? "Confirm" : "Delete", s->delete_note);
            button(s, top_button(s, 1), "Notes list", false);
            button(s, top_button(s, 0), s->note.editing_name ? "Start" : "Save", true);
        } else button(s, top_button(s, 0), "New", true);
    }
    if (s->app == FILES) button(s, top_button(s, 0), "Refresh", false);
    if (s->app == TERMINAL) {
        button(s, top_button(s, 1), s->term.fd >= 0 ? "Ctrl+C" : "New", false);
        button(s, top_button(s, 0), s->keyboard ? "Hide keys" : "Keyboard", true);
    }
    if (s->app == NETWORK)
        button(s, top_button(s, 0), s->wifi_edit ? "Connect" : "Scan", true);
    if (s->app == AUTH && !s->login.setup)
        button(s, top_button(s, 0), s->login.add ? "Cancel" : "Add user", false);
    if (s->keyboard) {
        box(d, rect(0, bottom(s), d->w, d->h - NAV - bottom(s)), c.dim);
        SoftKey keys[52];
        int count = keyboard_layout(s, keys);
        for (int i = 0; i < count; i++) {
            SoftKey key = keys[i];
            char label[] = { (char)key.key, 0 };
            button(s, key.box, key.label ? key.label : label,
                   key.key == '\n' || (key.key == SHIFT_UI && s->shift) ||
                   (key.key == SYMBOL_UI && s->symbols) || (key.key == CTRL_UI && s->control));
        }
    }
    if (home) {
        int width = min(d->w - 24, 430);
        round_box(d, rect((d->w - width) / 2, d->h - NAV + 2, width, NAV - 6), 23, 0x23354b);
    } else {
        box(d, rect(0, d->h - NAV, d->w, NAV), c.nav);
        box(d, rect(0, d->h - NAV, d->w, 1), c.edge);
    }
    if (s->app == AUTH) {
        if (s->login.field > 0)
            button(s, auth_toggle(s), s->login.show ? "Hide password" : "Show password", false);
        button(s, auth_action(s), (s->login.setup || s->login.add) ? "Create account" : "Sign in", true);
    } else {
        for (int i = 0; i < 3; i++) {
            Rect r = nav_button(s, i);
            if (i == 0) {
                centered(d, r, "Home", FACE_REGULAR, home ? 0xffffff : c.ink);
            } else {
                int index = s->order[i - 1];
                icon(d, r.x + (r.w - 43) / 2, r.y, apps[index].app, apps[index].tint);
            }
        }
    }
    if (s->show_pointer) {
        circle(d, s->mx, s->my, 5, c.ink);
        circle(d, s->mx, s->my, 3, c.accent);
    }
}

void ui_draw(State *s)
{
    if (s->app == HOME) wallpaper(s->d, s->light);
    else box(s->d, rect(0, 0, s->d->w, s->d->h), colors(s).dim);
    switch (s->app) {
    case HOME: draw_home(s); break;
    case NOTES: draw_note(s); break;
    case TERMINAL: draw_terminal(s); break;
    case FILES: draw_list(s); break;
    case CALCULATOR: draw_calculator(s); break;
    case CLOCK: draw_clock(s); break;
    case SETTINGS: draw_settings(s); break;
    case NETWORK: draw_network(s); break;
    case AUTH: draw_auth(s); break;
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
    if (s->app == app) { s->drawer = s->tasks = false; return; }
    if (s->app == NOTES && !notes_save(s)) return;
    s->app = app;
    s->drawer = s->tasks = false;
    if (app != HOME && app != AUTH) {
        int found = -1;
        for (int i = 0; i < s->recent_count; i++) if (s->recent[i] == app) found = i;
        if (found >= 0) {
            for (int i = found; i > 0; i--) s->recent[i] = s->recent[i - 1];
        } else {
            if (s->recent_count < APP_COUNT) s->recent_count++;
            for (int i = s->recent_count - 1; i > 0; i--) s->recent[i] = s->recent[i - 1];
        }
        s->recent[0] = app;
    }
    s->scroll = 0;
    s->keyboard = s->shift = s->symbols = s->control = s->confirm_power = false;
    s->message[0] = 0;
    s->arranging = false;
    s->delete_note = false;
    s->selection = -1;
    s->wifi_edit = false;
    explicit_bzero(s->wifi_password, sizeof(s->wifi_password));
    if (app == NETWORK) network_status(s);
    s->list_dirty = true;
}

static int scroll_limit(State *s)
{
    switch (s->app) {
    case HOME: return 0;
    case NETWORK: return s->wifi_edit ? 0 : max(0, (s->wifi_count - 1) * 49);
    case SETTINGS: return max(0, TOP + 320 - bottom(s));
    case NOTES:
        if (s->note.open)
            return max(0, line_count(s->note.text, s->note.length, text_cols(s)) - note_rows(s));
        /* fall through */
    case FILES:
        return max(0, TOP + 8 + s->name_count * 55 - bottom(s));
    case TERMINAL:
        return terminal_history(s);
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

static void wifi_key(State *s, int key)
{
    if (!s->wifi_edit) return;
    size_t length = strlen(s->wifi_password);
    if (key == '\n') network_connect(s);
    else if (key == '\b' && length) s->wifi_password[length - 1] = 0;
    else if (key >= 32 && key <= 126 && length < 63) {
        s->wifi_password[length] = (char)key;
        s->wifi_password[length + 1] = 0;
    }
}

void ui_key(State *s, int key)
{
    if (key == NO_KEY) return;
    if (s->app == AUTH) {
        if (key == KEY_ESCAPE_UI) { s->keyboard = false; s->login.show = false; }
        else account_key(s, key);
    } else if (s->app == TERMINAL) {
        terminal_key(s, key);
    } else if (key == KEY_HOME_UI || key == KEY_ESCAPE_UI) {
        go(s, HOME);
    } else if (key == KEY_SAVE_UI) {
        notes_save(s);
    } else switch (s->app) {
    case NOTES: note_key(s, key); break;
    case NETWORK: wifi_key(s, key); break;
    case CALCULATOR: calc_key(s, key); break;
    case HOME:
        if (key == KEY_LEFT_UI) s->page = max(0, s->page - 1);
        if (key == KEY_RIGHT_UI) s->page = min(PAGE_COUNT - 1, s->page + 1);
        break;
    case FILES: case SETTINGS:
        if (key == KEY_UP_UI || key == KEY_DOWN_UI) ui_scroll(s, key == KEY_UP_UI ? -65 : 65);
        break;
    default: break;
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

static void save_layout(const State *s)
{
    if (!*s->data) return;
    char scratch[PATH_MAX], final[PATH_MAX];
    if (snprintf(scratch, sizeof(scratch), "%s/.layout-XXXXXX", s->data) >= (int)sizeof(scratch) ||
        snprintf(final, sizeof(final), "%s/.layout", s->data) >= (int)sizeof(final)) return;
    int fd = mkostemp(scratch, O_CLOEXEC);
    if (fd < 0) return;
    bool good = true;
    for (int i = 0; i < APP_COUNT; i++)
        if (dprintf(fd, "%d\n", s->order[i]) < 0) good = false;
    if (fsync(fd)) good = false;
    if (close(fd)) good = false;
    if (!good || rename(scratch, final)) unlink(scratch);
}

static void click(State *s)
{
    if (s->app == AUTH) {
        if (hit(s, auth_toggle(s)) && s->login.field > 0) { s->login.show = !s->login.show; return; }
        if (hit(s, auth_action(s))) { account_submit(s); return; }
    } else {
        for (int i = 0; i < 3; i++) {
            if (hit(s, nav_button(s, i))) { go(s, i == 0 ? HOME : apps[s->order[i - 1]].app); return; }
        }
    }
    if (s->keyboard && s->my >= bottom(s)) {
        SoftKey keys[52];
        int n = keyboard_layout(s, keys);
        for (int i = 0; i < n; i++) {
            if (!hit(s, keys[i].box)) continue;
            int key = keys[i].key;
            if (key == SHIFT_UI) s->shift = !s->shift;
            else if (key == SYMBOL_UI) s->symbols = !s->symbols;
            else if (key == CTRL_UI) s->control = !s->control;
            else if (key == HIDE_UI) {
                s->keyboard = false;
                s->scroll = min(s->scroll, scroll_limit(s));
            } else {
                int lower = key >= 'A' && key <= 'Z' ? key - 'A' + 'a' : key;
                if (s->control && lower >= 'a' && lower <= 'z') key = KEY_CONTROL_UI + lower - 'a';
                ui_key(s, key);
                s->shift = s->control = false;
            }
            return;
        }
        return;
    }
    if (s->my < TOP) {
        if (s->app == HOME && hit(s, top_button(s, 0))) {
            s->arranging = !s->arranging;
            s->selection = -1;
        } else if (s->app == AUTH && !s->login.setup && hit(s, top_button(s, 0))) {
            bool add = !s->login.add;
            explicit_bzero(&s->login, sizeof(s->login));
            s->login.add = add;
            s->message[0] = 0;
        } else if (s->app == NETWORK && hit(s, top_button(s, 0))) {
            if (s->wifi_edit) network_connect(s);
            else network_scan(s);
        } else if (s->app == NOTES) {
            if (s->note.open && hit(s, top_button(s, 2))) {
                if (s->delete_note) { notes_delete(s); s->delete_note = false; }
                else { s->delete_note = true; strcpy(s->message, "Tap Confirm to delete this note"); }
            } else if (hit(s, top_button(s, 0))) {
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
                s->control = false;
                s->scroll = 0;
            }
            if (hit(s, top_button(s, 1))) {
                if (s->term.fd >= 0) terminal_key(s, KEY_BREAK_UI);
                else terminal_close(&s->term);
            }
        }
        return;
    }
    if (s->my >= bottom(s)) return;
    switch (s->app) {
    case HOME:
        if (s->drawer || s->tasks) {
            int cols = columns(s), step = (s->d->w - 42) / cols;
            int count = s->tasks ? s->recent_count : APP_COUNT;
            for (int i = 0; i < count; i++) {
                Rect tile = rect(21 + i % cols * step, TOP + 57 + i / cols * 96, step - 5, 91);
                if (hit(s, tile)) { go(s, s->tasks ? s->recent[i] : apps[s->order[i]].app); return; }
            }
            return;
        }
        for (int p = 0; p < PAGE_COUNT; p++)
            if (hit(s, page_dot(s, p))) { s->page = p; return; }
        for (int i = 0; i < APP_COUNT; i++) {
            if (!hit(s, app_card(s, i))) continue;
            if (!s->arranging) go(s, apps[s->order[i]].app);
            else if (s->selection == i) s->selection = -1;
            else if (s->selection < 0) s->selection = i;
            else {
                int other = s->order[s->selection];
                s->order[s->selection] = s->order[i];
                s->order[i] = other;
                s->selection = -1;
                save_layout(s);
            }
            break;
        }
        break;
    case AUTH:
        for (int i = 0; i < auth_fields(s); i++)
            if (hit(s, auth_field(s, i))) {
                s->login.field = i;
                s->keyboard = true;
                s->login.show = false;
                break;
            }
        break;
    case NETWORK:
        if (s->wifi_edit) { s->keyboard = true; break; }
        if (s->my < TOP + 70 || s->my >= bottom(s) - 23) break;
        for (int i = 0; i < s->wifi_count; i++)
            if (hit(s, network_row(s, i))) {
                strcpy(s->wifi_ssid, s->wifi_results[i]);
                s->wifi_open = !s->wifi_secured[i];
                s->wifi_edit = true;
                s->scroll = 0;
                s->keyboard = !s->wifi_open;
                s->message[0] = 0;
                if (s->wifi_open) network_connect(s);
                break;
            }
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
        if (hit(s, terminal_area(s))) { s->keyboard = true; s->scroll = 0; }
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
        else if (hit(s, setting_button(s, 1))) go(s, NETWORK);
        else if (getenv("KOS_APPLIANCE") && hit(s, setting_button(s, 2)) && notes_save(s)) {
            s->exit_code = 43;
            s->running = false;
        } else if (hit(s, setting_button(s, getenv("KOS_APPLIANCE") ? 3 : 2)) && notes_save(s)) {
            if (s->confirm_power) { s->running = false; s->exit_code = 42; }
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
        s->page_start = s->page_position;
        s->press_at = s->now;
        s->dragged = s->swipe_horizontal = false;
    }
    if (s->pointer) {
        int dx = s->mx - s->start_x, dy = s->my - s->start_y;
        if (!s->dragged && (abs(dx) > 8 || abs(dy) > 8)) {
            s->dragged = true;
            s->swipe_horizontal = s->app == HOME && abs(dx) > abs(dy);
        }
        bool content = s->start_y >= TOP && s->start_y < bottom(s);
        if (!down && s->dragged && dy < -45 && abs(dy) > abs(dx)) {
            if (s->app == HOME) { s->drawer = true; s->tasks = false; }
            else if (s->start_y >= s->d->h - NAV - 45) {
                if (s->app == NOTES && !notes_save(s)) return;
                s->app = HOME; s->tasks = true; s->drawer = false; s->keyboard = false;
            }
            s->pointer = false; s->redraw = true; return;
        }
        if (!down && s->dragged && dy > 45 && s->app == HOME && (s->drawer || s->tasks)) {
            s->drawer = s->tasks = false; s->pointer = false; s->redraw = true; return;
        }
        if (s->dragged && content && s->swipe_horizontal) {
            double distance = (double)dx / s->d->w;
            double target = s->page_start - distance;
            if (target < 0) target /= 4;
            if (target > PAGE_COUNT - 1) target = PAGE_COUNT - 1 + (target - PAGE_COUNT + 1) / 4;
            s->page_position = target;
            if (!down) {
                int threshold = (s->now - s->press_at < 320) ? 36 : s->d->w / 5;
                int page = (int)lround(s->page_start);
                if (abs(dx) > threshold) page += dx < 0 ? 1 : -1;
                s->page = max(0, min(PAGE_COUNT - 1, page));
            }
        } else if (s->dragged && content && s->app != HOME && s->app != AUTH) {
            s->scroll = s->start_scroll;
            ui_scroll(s, -dy);
        } else if (!down && !s->dragged) {
            if (s->app == HOME && !s->arranging && content && s->now - s->press_at > 500) {
                for (int i = 0; i < APP_COUNT; i++) if (hit(s, app_card(s, i))) {
                    s->arranging = true;
                    s->selection = i;
                    s->pointer = false;
                    s->redraw = true;
                    return;
                }
            }
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
    s->light = true;
    s->selection = -1;
    for (int i = 0; i < APP_COUNT; i++) s->order[i] = i;
    const char *user = getenv("USER");
    snprintf(s->user, sizeof(s->user), "%.32s", user && *user ? user : "local");
    if (data && strlen(data) < sizeof(s->data) - 14) {
        strcpy(s->data, data);
        char path[PATH_MAX];
        size_t len = strlen(data);
        memcpy(path, data, len);
        memcpy(path + len, "/.appearance", sizeof("/.appearance"));
        int fd = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK);
        struct stat st;
        FILE *file = fd >= 0 && fstat(fd, &st) == 0 && S_ISREG(st.st_mode) ? fdopen(fd, "r") : NULL;
        if (file) {
            char choice[8];
            if (fgets(choice, sizeof(choice), file)) s->light = strcmp(choice, "dark\n") != 0;
            fclose(file);
        } else if (fd >= 0) close(fd);
        memcpy(path + len, "/.layout", sizeof("/.layout"));
        fd = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK);
        file = fd >= 0 && fstat(fd, &st) == 0 && S_ISREG(st.st_mode) ? fdopen(fd, "r") : NULL;
        if (file) {
            int layout[APP_COUNT], mask = 0;
            for (int i = 0; i < APP_COUNT; i++) {
                if (fscanf(file, "%d", &layout[i]) != 1 || layout[i] < 0 ||
                    layout[i] >= APP_COUNT || (mask & (1 << layout[i]))) break;
                mask |= 1 << layout[i];
            }
            if (mask == (1 << APP_COUNT) - 1) memcpy(s->order, layout, sizeof(layout));
            fclose(file);
        } else if (fd >= 0) close(fd);
    }
}

void ui_tick(State *s)
{
    unsigned now = milliseconds(), before = s->now;
    s->now = now;
    if (s->app == CLOCK && before / 100 != now / 100) s->redraw = true;
    if (s->app == HOME && before / 1000 != now / 1000) s->redraw = true;
    if (s->app == HOME && !(s->pointer && s->swipe_horizontal) && s->page_position != s->page) {
        double delta = s->page - s->page_position;
        if (fabs(delta) < .003) s->page_position = s->page;
        else s->page_position += delta * min((int)(now - before), 28) / 90.;
        s->redraw = true;
    }
    if (s->app == NETWORK) {
        if (before / 1000 != now / 1000 && network_status(s)) s->redraw = true;
        network_refresh(s);
        network_join_update(s);
    }
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
    failure = "horizontal swipe and persistent home arrangement";
    ui_pointer(&s, 290, 350, true, false);
    ui_pointer(&s, 55, 350, true, false);
    ui_pointer(&s, 55, 350, false, false);
    if (s.page != 1 || s.page_position < .5 || s.app != HOME) goto fail;
    s.page = 0;
    s.page_position = 0;
    tap(&s, top_button(&s, 0));
    tap(&s, app_card(&s, 2));
    tap(&s, app_card(&s, 3));
    if (s.order[2] != 3 || s.order[3] != 2) goto fail;
    static State loaded;
    ui_init(&loaded, &d, test_dir);
    if (loaded.order[2] != 3 || loaded.order[3] != 2) goto fail;
    tap(&s, top_button(&s, 0));
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
    failure = "delete a note with confirmation";
    tap(&s, top_button(&s, 2));
    if (!s.delete_note || !s.note.open) goto fail;
    tap(&s, top_button(&s, 2));
    if (s.note.open || s.delete_note) goto fail;
    char deleted[256];
    snprintf(deleted, sizeof(deleted), "%s/note.txt", test_dir);
    if (access(deleted, F_OK) == 0) goto fail;
    failure = "interactive PTY, pipeline and foreground stdin";
    tap(&s, nav_button(&s, 2));
    ui_draw(&s);
    type(&s, "test -t 0 && test -t 1 && printf '\\163hell works\\n' | cat\n");
    for (int i = 0; i < 150 && !terminal_contains(&s.term, "shell works"); i++) {
        usleep(10000);
        ui_tick(&s);
    }
    if (!terminal_contains(&s.term, "shell works")) goto fail;
    type(&s, "read -r word; printf 'reply=%s\\n' \"$word\"\n");
    usleep(45000);
    type(&s, "hiya\n");
    for (int i = 0; i < 150 && !terminal_contains(&s.term, "reply=hiya"); i++) {
        usleep(10000);
        ui_tick(&s);
    }
    if (!terminal_contains(&s.term, "reply=hiya")) goto fail;
    failure = NULL;
fail:
    terminal_close(&s.term);
    char file[256];
    snprintf(file, sizeof(file), "%s/note.txt", test_dir);
    unlink(file);
    snprintf(file, sizeof(file), "%s/.layout", test_dir);
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
