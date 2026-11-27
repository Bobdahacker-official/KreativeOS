#include "kos.h"

#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <unistd.h>

struct Font { const unsigned char *pixels, *advance; int w, h; };
#include "font.h"

static const struct Font *get_font(Face face)
{
    static const struct Font *fonts[] = { &regular, &strong, &display, &mono };
    return fonts[face];
}

static bool allocate(Display *d)
{
    int width = d->rotation % 180 ? d->device_h : d->device_w;
    int height = d->rotation % 180 ? d->device_w : d->device_h;
    const char *scale = getenv("KOS_SCALE");
    d->scale = scale ? atoi(scale) : 0;
    if (d->scale < 1 || d->scale > 4)
        d->scale = width >= 1400 && height >= 900 ? 2 : 1;
    d->w = width / d->scale;
    d->h = height / d->scale;
    if (d->w < 280 || d->h < 300 || width > 8192 || height > 8192) return false;
    d->pixels = calloc((size_t)d->w * d->h, sizeof(*d->pixels));
    return d->pixels != NULL;
}

bool display_memory(Display *d, int width, int height)
{
    *d = (Display){ .fd = -1, .device_w = width, .device_h = height };
    return allocate(d);
}

static bool valid_channel(struct fb_bitfield channel, unsigned bits)
{
    return channel.length > 0 && channel.length <= 16 && channel.offset < bits &&
           channel.length <= bits - channel.offset && !channel.msb_right;
}

bool display_open(Display *d, const char *path)
{
    *d = (Display){ .fd = -1 };
    d->fd = open(path, O_RDWR | O_CLOEXEC);
    if (d->fd < 0) return false;
    if (ioctl(d->fd, FBIOGET_FSCREENINFO, &d->fix) < 0 ||
        ioctl(d->fd, FBIOGET_VSCREENINFO, &d->var) < 0) goto fail;
    unsigned bits = d->var.bits_per_pixel;
    if (d->fix.type != FB_TYPE_PACKED_PIXELS || d->fix.visual != FB_VISUAL_TRUECOLOR ||
        (bits != 16 && bits != 24 && bits != 32) || !valid_channel(d->var.red, bits) ||
        !valid_channel(d->var.green, bits) || !valid_channel(d->var.blue, bits)) goto fail;
    uint64_t row = ((uint64_t)d->var.xoffset + d->var.xres) * (bits / 8);
    if (row > d->fix.line_length || !d->var.yres ||
        ((uint64_t)d->var.yoffset + d->var.yres - 1) * d->fix.line_length + row >
        d->fix.smem_len) goto fail;
    d->mapped_size = d->fix.smem_len;
    d->mapped = mmap(NULL, d->mapped_size, PROT_READ | PROT_WRITE, MAP_SHARED, d->fd, 0);
    if (d->mapped == MAP_FAILED) {
        d->mapped = NULL;
        goto fail;
    }
    d->device_w = (int)d->var.xres;
    d->device_h = (int)d->var.yres;
    const char *rotation = getenv("KOS_ROTATE");
    d->rotation = rotation ? atoi(rotation) : 0;
    if (d->rotation != 90 && d->rotation != 180 && d->rotation != 270) d->rotation = 0;
    if (allocate(d)) return true;
fail:
    display_close(d);
    return false;
}

void display_close(Display *d)
{
    if (d->mapped) munmap(d->mapped, d->mapped_size);
    if (d->fd >= 0) close(d->fd);
    free(d->pixels);
}

static int clamp(int value, int low, int high)
{
    if (value < low) return low;
    if (value > high) return high;
    return value;
}

void display_point(const Display *d, int x, int y, int *out_x, int *out_y)
{
    int lx = x, ly = y;
    switch (d->rotation) {
    case 90: lx = y; ly = d->device_w - 1 - x; break;
    case 180: lx = d->device_w - 1 - x; ly = d->device_h - 1 - y; break;
    case 270: lx = d->device_h - 1 - y; ly = x; break;
    }
    *out_x = clamp(lx / d->scale, 0, d->w - 1);
    *out_y = clamp(ly / d->scale, 0, d->h - 1);
}

static uint32_t channel(unsigned value, struct fb_bitfield field)
{
    return ((value * ((1u << field.length) - 1) + 127) / 255) << field.offset;
}

void display_present(Display *d)
{
    if (!d->mapped) return;
    unsigned bytes = d->var.bits_per_pixel / 8;
    bool rgb = d->var.red.offset == 16 && d->var.green.offset == 8 && d->var.blue.offset == 0 &&
        d->var.red.length == 8 && d->var.green.length == 8 && d->var.blue.length == 8;
    for (int y = 0; y < d->device_h; y++) {
        unsigned char *dst = d->mapped + (y + d->var.yoffset) * d->fix.line_length +
                             d->var.xoffset * bytes;
        if (rgb && bytes == 4 && d->scale == 1 && d->rotation == 0) {
            memcpy(dst, d->pixels + (size_t)y * d->w, (size_t)d->w * 4);
            continue;
        }
        for (int x = 0; x < d->device_w; x++, dst += bytes) {
            int lx, ly;
            display_point(d, x, y, &lx, &ly);
            uint32_t color = d->pixels[(size_t)ly * d->w + lx];
            if (!rgb) color = channel((color >> 16) & 255, d->var.red) |
                              channel((color >> 8) & 255, d->var.green) |
                              channel(color & 255, d->var.blue);
            memcpy(dst, &color, bytes);
        }
    }
}

bool display_dump(Display *d, const char *path)
{
    FILE *file = fopen(path, "wb");
    if (!file) return false;
    fprintf(file, "P6\n%d %d\n255\n", d->w, d->h);
    for (size_t i = 0; i < (size_t)d->w * d->h; i++) {
        uint32_t color = d->pixels[i];
        unsigned char rgb[] = { color >> 16, color >> 8, color };
        fwrite(rgb, 1, 3, file);
    }
    bool good = !ferror(file);
    return fclose(file) == 0 && good;
}

Rect rect(int x, int y, int w, int h) { return (Rect){ x, y, w, h }; }

bool inside(Rect r, int x, int y)
{
    return x >= r.x && y >= r.y && x < r.x + r.w && y < r.y + r.h;
}

void box(Display *d, Rect r, uint32_t color)
{
    int left = clamp(r.x, 0, d->w), right = clamp(r.x + r.w, 0, d->w);
    int top = clamp(r.y, 0, d->h), bottom = clamp(r.y + r.h, 0, d->h);
    for (int y = top; y < bottom; y++)
        for (int x = left; x < right; x++) d->pixels[(size_t)y * d->w + x] = color;
}

void round_box(Display *d, Rect r, int radius, uint32_t color)
{
    if (radius > r.w / 2) radius = r.w / 2;
    if (radius > r.h / 2) radius = r.h / 2;
    if (radius <= 0) {
        box(d, r, color);
        return;
    }
    for (int y = 0; y < r.h; y++) {
        int dy = (y < r.h / 2 ? y : r.h - y - 1);
        int cut = 0;
        if (dy < radius) {
            int rise = radius - dy - 1;
            while ((radius - cut) * (radius - cut) + rise * rise > radius * radius) cut++;
        }
        box(d, rect(r.x + cut, r.y + y, r.w - cut * 2, 1), color);
    }
}

void line(Display *d, int x, int y, int end_x, int end_y, uint32_t color)
{
    int dx = abs(end_x - x), sx = x < end_x ? 1 : -1;
    int dy = -abs(end_y - y), sy = y < end_y ? 1 : -1;
    int error = dx + dy;
    for (;;) {
        if (x >= 0 && y >= 0 && x < d->w && y < d->h)
            d->pixels[(size_t)y * d->w + x] = color;
        if (x == end_x && y == end_y) break;
        int step = 2 * error;
        if (step >= dy) { error += dy; x += sx; }
        if (step <= dx) { error += dx; y += sy; }
    }
}

void circle(Display *d, int x, int y, int r, uint32_t color)
{
    for (int dy = -r; dy <= r; dy++)
        for (int dx = -r; dx <= r; dx++)
            if (dx * dx + dy * dy <= r * r) box(d, rect(x + dx, y + dy, 1, 1), color);
}

void text(Display *d, int x, int y, const char *str, Face face, uint32_t color, int right)
{
    const struct Font *font = get_font(face);
    for (; *str && x < right; str++) {
        unsigned c = (unsigned char)*str;
        if (c < 32 || c > 126) c = '?';
        const unsigned char *glyph = font->pixels + (c - 32) * font->w * font->h;
        for (int row = 0; row < font->h; row++) {
            if (y + row < 0 || y + row >= d->h) continue;
            for (int column = 0; column < font->w; column++)
                if (x + column >= 0 && x + column < right && x + column < d->w) {
                    unsigned alpha = glyph[row * font->w + column];
                    if (!alpha) continue;
                    uint32_t *pixel = &d->pixels[(size_t)(y + row) * d->w + x + column];
                    if (alpha == 15) {
                        *pixel = color;
                    } else {
                        uint32_t bg = *pixel, mixed = 0;
                        for (unsigned shift = 0; shift <= 16; shift += 8) {
                            unsigned channel = ((color >> shift) & 255) * alpha +
                                               ((bg >> shift) & 255) * (15 - alpha);
                            mixed |= ((channel + 7) / 15) << shift;
                        }
                        *pixel = mixed;
                    }
                }
        }
        x += font->advance[c - 32];
    }
}

int text_width(const char *str, Face face)
{
    const struct Font *font = get_font(face);
    int width = 0;
    for (; *str; str++) {
        unsigned c = (unsigned char)*str;
        width += font->advance[c >= 32 && c <= 126 ? c - 32 : '?' - 32];
    }
    return width;
}

void wallpaper(Display *d, bool light)
{
    for (int y = 0; y < d->h; y++)
        for (int x = 0; x < d->w; x++) {
            int a = (d->w - x) * 65 / d->w;
            int b = (d->h - y) * 45 / d->h;
            int glow = a * b / 65;
            unsigned r = light ? 231 + glow / 3 : 15 + glow;
            unsigned g = light ? 236 + glow / 4 : 19 + glow / 2;
            unsigned blue = light ? 245 + glow / 5 : 36 + glow * 4 / 3;
            d->pixels[(size_t)y * d->w + x] = r << 16 | g << 8 | blue;
        }
}
