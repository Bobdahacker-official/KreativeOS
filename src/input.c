#include "kos.h"

#include <dirent.h>
#include <fcntl.h>
#include <poll.h>
#include <stdio.h>
#include <string.h>
#include <sys/inotify.h>
#include <sys/ioctl.h>
#include <unistd.h>

#define BITS(n) (((n) + 8) / 8)
static bool bit(const unsigned char *bits, unsigned n) { return bits[n / 8] & (1u << (n % 8)); }

static void discover(Inputs *inputs)
{
    for (int i = 0; i < inputs->count; i++) close(inputs->items[i].fd);
    inputs->count = 0;
    DIR *directory = opendir("/dev/input");
    if (!directory) return;
    struct dirent *entry;
    while (inputs->count < (int)ARRAY_LEN(inputs->items) && (entry = readdir(directory))) {
        if (strncmp(entry->d_name, "event", 5) != 0) continue;
        char path[sizeof(entry->d_name) + 12];
        snprintf(path, sizeof(path), "/dev/input/%s", entry->d_name);
        int fd = open(path, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
        if (fd < 0) continue;
        unsigned char keys[BITS(KEY_MAX)] = {0}, props[BITS(INPUT_PROP_MAX)] = {0};
        unsigned char axes[BITS(ABS_MAX)] = {0}, relative[BITS(REL_MAX)] = {0};
        ioctl(fd, EVIOCGBIT(EV_KEY, sizeof(keys)), keys);
        ioctl(fd, EVIOCGBIT(EV_ABS, sizeof(axes)), axes);
        ioctl(fd, EVIOCGBIT(EV_REL, sizeof(relative)), relative);
        ioctl(fd, EVIOCGPROP(sizeof(props)), props);
        InputDevice device = {
            .fd = fd,
            .direct = bit(props, INPUT_PROP_DIRECT) || bit(keys, BTN_TOUCH),
            .has_mouse = bit(relative, REL_X) && bit(relative, REL_Y),
            .has_keyboard = bit(keys, KEY_A) && bit(keys, KEY_ENTER),
        };
        device.mt = device.direct && bit(axes, ABS_MT_POSITION_X) &&
                    bit(axes, ABS_MT_POSITION_Y) && bit(axes, ABS_MT_SLOT);
        unsigned x_axis = device.mt ? ABS_MT_POSITION_X : ABS_X;
        unsigned y_axis = device.mt ? ABS_MT_POSITION_Y : ABS_Y;
        device.absolute = bit(axes, x_axis) && bit(axes, y_axis) &&
                          (device.direct || bit(keys, BTN_LEFT)) &&
                          ioctl(fd, EVIOCGABS(x_axis), &device.ax) == 0 &&
                          ioctl(fd, EVIOCGABS(y_axis), &device.ay) == 0 &&
                          device.ax.minimum < device.ax.maximum && device.ay.minimum < device.ay.maximum;
        if (!device.absolute && !device.has_mouse && !device.has_keyboard) {
            close(fd);
            continue;
        }
        device.x = device.ax.value;
        device.y = device.ay.value;
        inputs->items[inputs->count++] = device;
    }
    closedir(directory);
}

bool inputs_open(Inputs *inputs)
{
    inputs->count = 0;
    inputs->monitor = inotify_init1(IN_NONBLOCK | IN_CLOEXEC);
    if (inputs->monitor >= 0 &&
        inotify_add_watch(inputs->monitor, "/dev/input", IN_CREATE | IN_DELETE | IN_ATTRIB) < 0) {
        close(inputs->monitor);
        inputs->monitor = -1;
    }
    discover(inputs);
    return inputs->monitor >= 0;
}

void inputs_close(Inputs *inputs)
{
    for (int i = 0; i < inputs->count; i++) close(inputs->items[i].fd);
    if (inputs->monitor >= 0) close(inputs->monitor);
}

static int to_ui(InputDevice *d, unsigned code)
{
    bool ctrl = d->ctrl_left || d->ctrl_right;
    bool shift = d->shift_left || d->shift_right;
    if (ctrl && code == KEY_S) return KEY_SAVE_UI;
    if (ctrl && code == KEY_C) return KEY_BREAK_UI;
    if (ctrl && code >= KEY_Q && code <= KEY_P) return KEY_CONTROL_UI + "qwertyuiop"[code - KEY_Q] - 'a';
    if (ctrl && code >= KEY_A && code <= KEY_L) return KEY_CONTROL_UI + "asdfghjkl"[code - KEY_A] - 'a';
    if (ctrl && code >= KEY_Z && code <= KEY_M) return KEY_CONTROL_UI + "zxcvbnm"[code - KEY_Z] - 'a';
    if (code >= KEY_Q && code <= KEY_P) return (shift ? "QWERTYUIOP" : "qwertyuiop")[code - KEY_Q];
    if (code >= KEY_A && code <= KEY_L) return (shift ? "ASDFGHJKL" : "asdfghjkl")[code - KEY_A];
    if (code >= KEY_Z && code <= KEY_M) return (shift ? "ZXCVBNM" : "zxcvbnm")[code - KEY_Z];
    if (code >= KEY_1 && code <= KEY_0) return (shift ? "!@#$%^&*()" : "1234567890")[code - KEY_1];
    switch (code) {
    case KEY_ESC: return KEY_ESCAPE_UI;
    case KEY_HOME: return KEY_HOME_UI;
    case KEY_UP: return KEY_UP_UI;
    case KEY_DOWN: return KEY_DOWN_UI;
    case KEY_LEFT: return KEY_LEFT_UI;
    case KEY_RIGHT: return KEY_RIGHT_UI;
    case KEY_ENTER: case KEY_KPENTER: return '\n';
    case KEY_TAB: return '\t';
    case KEY_SPACE: return ' ';
    case KEY_BACKSPACE: return '\b';
    case KEY_DELETE: return KEY_DELETE_UI;
    case KEY_MINUS: return shift ? '_' : '-';
    case KEY_EQUAL: return shift ? '+' : '=';
    case KEY_LEFTBRACE: return shift ? '{' : '[';
    case KEY_RIGHTBRACE: return shift ? '}' : ']';
    case KEY_SEMICOLON: return shift ? ':' : ';';
    case KEY_APOSTROPHE: return shift ? '"' : '\'';
    case KEY_GRAVE: return shift ? '~' : '`';
    case KEY_BACKSLASH: return shift ? '|' : '\\';
    case KEY_COMMA: return shift ? '<' : ',';
    case KEY_DOT: return shift ? '>' : '.';
    case KEY_SLASH: return shift ? '?' : '/';
    default: return NO_KEY;
    }
}

static int position(struct input_absinfo range, int value, int size)
{
    if (value <= range.minimum) return 0;
    if (value >= range.maximum) return size - 1;
    return (int)(((int64_t)value - range.minimum) * (size - 1) /
                 ((int64_t)range.maximum - range.minimum));
}

static void report(InputDevice *device, State *s)
{
    int x = s->mx, y = s->my;
    if (device->absolute) {
        display_point(s->d, position(device->ax, device->x, s->d->device_w),
                      position(device->ay, device->y, s->d->device_h), &x, &y);
    } else {
        int dx = device->dx, dy = device->dy;
        switch (s->d->rotation) {
        case 90: dx = device->dy; dy = -device->dx; break;
        case 180: dx = -device->dx; dy = -device->dy; break;
        case 270: dx = -device->dy; dy = device->dx; break;
        }
        x += dx;
        y += dy;
    }
    if (device->changed || device->holding != device->button) {
        ui_pointer(s, x, y, device->button, !device->direct);
        device->holding = device->button;
    }
    if (device->wheel) ui_scroll(s, -device->wheel * 45);
    device->wheel = device->dx = device->dy = 0;
    device->changed = false;
}

static void event(InputDevice *d, State *s, const struct input_event *ev)
{
    if (ev->type == EV_SYN) {
        if (ev->code == SYN_DROPPED) {
            d->dropped = true;
            d->shift_left = d->shift_right = d->ctrl_left = d->ctrl_right = false;
        }
        if (ev->code == SYN_REPORT && d->dropped) {
            d->dropped = false;
            d->button = d->holding = d->changed = false;
            d->wheel = d->dx = d->dy = 0;
            s->pointer = false;
        } else if (ev->code == SYN_REPORT) {
            report(d, s);
        }
    } else if (d->dropped) {
        return;
    } else if (ev->type == EV_ABS && d->absolute) {
        if (ev->code == ABS_MT_SLOT) d->slot = ev->value;
        if (d->mt && d->slot != 0) return;
        if (d->mt && ev->code == ABS_MT_TRACKING_ID) d->button = ev->value >= 0;
        if (ev->code == (d->mt ? ABS_MT_POSITION_X : ABS_X)) {
            d->x = ev->value;
            d->changed = true;
        }
        if (ev->code == (d->mt ? ABS_MT_POSITION_Y : ABS_Y)) {
            d->y = ev->value;
            d->changed = true;
        }
    } else if (ev->type == EV_REL && (d->has_mouse || d->absolute)) {
        if (ev->code == REL_X) { d->dx += ev->value; d->changed = true; }
        if (ev->code == REL_Y) { d->dy += ev->value; d->changed = true; }
        if (ev->code == REL_WHEEL) d->wheel += ev->value;
    } else if (ev->type == EV_KEY) {
        bool enabled = ev->value != 0;
        if (ev->code == BTN_LEFT || (ev->code == BTN_TOUCH && !d->mt)) d->button = enabled;
        if (!d->has_keyboard) return;
        switch (ev->code) {
        case KEY_LEFTSHIFT: d->shift_left = enabled; break;
        case KEY_RIGHTSHIFT: d->shift_right = enabled; break;
        case KEY_LEFTCTRL: d->ctrl_left = enabled; break;
        case KEY_RIGHTCTRL: d->ctrl_right = enabled; break;
        default: if (enabled) ui_key(s, to_ui(d, ev->code)); break;
        }
    }
}

void inputs_poll(Inputs *inputs, State *s)
{
    struct pollfd fds[ARRAY_LEN(inputs->items) + 1];
    fds[0] = (struct pollfd){ .fd = inputs->monitor, .events = POLLIN };
    for (int i = 0; i < inputs->count; i++)
        fds[i + 1] = (struct pollfd){ .fd = inputs->items[i].fd, .events = POLLIN };
    if (poll(fds, inputs->count + 1, 16) <= 0) return;
    if (fds[0].revents & POLLIN) {
        char buffer[4096];
        while (read(inputs->monitor, buffer, sizeof(buffer)) > 0) {}
        s->pointer = false;
        discover(inputs);
        return;
    }
    for (int i = 0; i < inputs->count; i++) {
        if (!(fds[i + 1].revents & POLLIN)) continue;
        struct input_event events[32];
        ssize_t bytes = read(inputs->items[i].fd, events, sizeof(events));
        for (ssize_t j = 0; bytes > 0 && j < bytes / (ssize_t)sizeof(*events); j++)
            event(&inputs->items[i], s, &events[j]);
    }
}
