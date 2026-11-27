#ifndef KOS_H
#define KOS_H

#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <limits.h>
#include <time.h>
#include <sys/types.h>
#include <linux/input.h>
#include <linux/fb.h>

#define NOTE_CAP 16384
#define NOTE_COUNT 128
#define OUTPUT_CAP 32768
#define ARRAY_LEN(a) (sizeof(a) / sizeof((a)[0]))

typedef struct { int x, y, w, h; } Rect;
typedef struct {
    int fd, w, h, scale, device_w, device_h, rotation;
    size_t mapped_size;
    unsigned char *mapped;
    uint32_t *pixels;
    struct fb_fix_screeninfo fix;
    struct fb_var_screeninfo var;
} Display;

typedef enum { FACE_REGULAR, FACE_STRONG, FACE_DISPLAY, FACE_MONO } Face;
typedef enum { HOME, NOTES, TERMINAL, FILES, CALCULATOR, CLOCK, SETTINGS } App;
typedef enum { NO_KEY = 0, KEY_UP_UI = 256, KEY_DOWN_UI, KEY_LEFT_UI, KEY_RIGHT_UI,
               KEY_HOME_UI, KEY_SAVE_UI, KEY_BREAK_UI, KEY_DELETE_UI } UiKey;

typedef struct {
    char name[48], text[NOTE_CAP + 1];
    size_t length, cursor;
    bool open, dirty, editing_name, creating;
    unsigned next_save;
} Note;

typedef struct {
    char line[512], output[OUTPUT_CAP], history[16][512], pending[512], cwd[PATH_MAX];
    size_t output_len, used, cursor;
    int fd, history_size, history_pos;
    pid_t pid;
    bool stream_done, exited;
    int status;
} Terminal;

typedef struct {
    App app;
    Display *d;
    bool light, keyboard, shift, symbols, running, redraw, pointer, show_pointer, list_dirty;
    int scroll, mx, my, start_x, start_y, start_scroll;
    bool dragged;
    unsigned now;
    char data[PATH_MAX], message[96];
    Note note;
    Terminal term;
    char names[NOTE_COUNT][48];
    int name_count;
    char expression[128], result[96];
    unsigned stopwatch_start, stopwatch_saved;
    bool stopwatch_running, confirm_power;
} State;

typedef struct {
    int fd, slot;
    bool mt, direct, absolute, holding, button, has_mouse, shift_left, shift_right;
    bool ctrl_left, ctrl_right, has_keyboard, changed, dropped;
    int x, y, dx, dy, wheel;
    struct input_absinfo ax, ay;
} InputDevice;
typedef struct { InputDevice items[48]; int count; int monitor; } Inputs;

bool display_open(Display *, const char *);
bool display_memory(Display *, int, int);
void display_close(Display *);
void display_present(Display *);
bool display_dump(Display *, const char *);
void display_point(const Display *, int, int, int *, int *);
Rect rect(int, int, int, int);
bool inside(Rect, int, int);
void box(Display *, Rect, uint32_t);
void round_box(Display *, Rect, int, uint32_t);
void line(Display *, int, int, int, int, uint32_t);
void circle(Display *, int, int, int, uint32_t);
void text(Display *, int, int, const char *, Face, uint32_t, int);
int text_width(const char *, Face);
void wallpaper(Display *, bool);

bool inputs_open(Inputs *);
void inputs_poll(Inputs *, State *);
void inputs_close(Inputs *);

bool notes_save(State *);
void notes_list(State *);
void notes_open(State *, const char *);
void notes_new(State *);
void terminal_key(State *, int);
void terminal_poll(State *);
bool calculate(const char *, double *);
void ui_init(State *, Display *, const char *);
void ui_draw(State *);
void ui_tick(State *);
void ui_key(State *, int);
void ui_pointer(State *, int, int, bool, bool);
void ui_scroll(State *, int);
int ui_test(void);

#endif
