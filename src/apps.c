#include "kos.h"

#include <ctype.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <grp.h>
#include <math.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/random.h>
#include <sys/prctl.h>
#include <sys/wait.h>
#include <unistd.h>

static int data_dir(const State *s)
{
    if (!*s->data) return -1;
    return open(s->data, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
}

static bool valid_name(const char *name)
{
    size_t len = strlen(name);
    if (!len || len >= 48 || name[0] == '.' || name[len - 1] == ' ') return false;
    for (const unsigned char *c = (const unsigned char *)name; *c; c++)
        if (!isalnum(*c) && *c != ' ' && *c != '-' && *c != '_') return false;
    return true;
}

static void note_file(char file[52], const char *name)
{
    snprintf(file, 52, "%s.txt", name);
}

static int name_order(const void *a, const void *b)
{
    return strcasecmp(a, b);
}

void notes_list(State *s)
{
    s->name_count = 0;
    s->list_dirty = false;
    int fd = data_dir(s);
    if (fd < 0) return;
    DIR *directory = fdopendir(fd);
    if (!directory) { close(fd); return; }
    struct dirent *entry;
    while (s->name_count < NOTE_COUNT && (entry = readdir(directory))) {
        size_t length = strlen(entry->d_name);
        if (length < 5 || length > 51 || strcmp(entry->d_name + length - 4, ".txt")) continue;
        struct stat st;
        if (fstatat(fd, entry->d_name, &st, AT_SYMLINK_NOFOLLOW) < 0 ||
            !S_ISREG(st.st_mode) || st.st_size < 0 || st.st_size > NOTE_CAP) continue;
        char *name = s->names[s->name_count];
        memcpy(name, entry->d_name, length - 4);
        name[length - 4] = 0;
        if (valid_name(name)) s->name_count++;
    }
    closedir(directory);
    qsort(s->names, s->name_count, sizeof(s->names[0]), name_order);
}

void notes_open(State *s, const char *name)
{
    if (!valid_name(name) || !notes_save(s)) return;
    int dir = data_dir(s);
    if (dir < 0) return;
    char file[52];
    note_file(file, name);
    int fd = openat(dir, file, O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK);
    close(dir);
    if (fd < 0) { strcpy(s->message, "Could not open this note"); return; }
    struct stat st;
    Note note = {0};
    bool ok = fstat(fd, &st) == 0 && S_ISREG(st.st_mode) && st.st_size >= 0 && st.st_size <= NOTE_CAP;
    while (ok && note.length < (size_t)st.st_size) {
        ssize_t count = read(fd, note.text + note.length, (size_t)st.st_size - note.length);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) { ok = false; break; }
        note.length += (size_t)count;
    }
    close(fd);
    if (!ok) { strcpy(s->message, "Could not read this note"); return; }
    strcpy(note.name, name);
    note.cursor = note.length;
    note.open = true;
    s->message[0] = 0;
    s->note = note;
    s->app = NOTES;
    s->scroll = 0;
}

void notes_new(State *s)
{
    if (!notes_save(s)) return;
    s->note = (Note){ .open = true, .editing_name = true, .creating = true };
    s->keyboard = true;
    s->scroll = 0;
    s->message[0] = 0;
}

bool notes_save(State *s)
{
    Note *note = &s->note;
    if (!note->open || !note->dirty) return true;
    if (!valid_name(note->name)) {
        strcpy(s->message, "Use letters, numbers, spaces, - or _");
        return false;
    }
    int dir = data_dir(s);
    if (dir < 0) {
        strcpy(s->message, "Storage is unavailable");
        return false;
    }
    char file[52], temp[64];
    note_file(file, note->name);
    unsigned nonce;
    if (getrandom(&nonce, sizeof(nonce), GRND_NONBLOCK) != sizeof(nonce))
        nonce = (unsigned)getpid() ^ s->now;
    snprintf(temp, sizeof(temp), ".note-%ld-%08x.tmp", (long)getpid(), nonce);
    int fd = openat(dir, temp, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0644);
    bool ok = fd >= 0;
    if (ok && geteuid() == 0 && fchown(fd, 65534, 65534) < 0) ok = false;
    for (size_t offset = 0; ok && offset < note->length;) {
        ssize_t count = write(fd, note->text + offset, note->length - offset);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) { ok = false; break; }
        offset += (size_t)count;
    }
    if (fd >= 0) {
        if (ok && fsync(fd) < 0) ok = false;
        if (close(fd) < 0) ok = false;
        if (ok) {
            int committed = note->creating ? renameat2(dir, temp, dir, file, RENAME_NOREPLACE) :
                                            renameat(dir, temp, dir, file);
            if (committed == 0) note->creating = false;
            ok = committed == 0 && fsync(dir) == 0;
        }
        if (!ok) unlinkat(dir, temp, 0);
    }
    close(dir);
    if (ok) {
        note->dirty = false;
        s->list_dirty = true;
        strcpy(s->message, getenv("KOS_VOLATILE") ? "Saved for this session" : "Saved");
    } else {
        strcpy(s->message, "Save failed. Your edits are still open.");
    }
    return ok;
}

typedef struct { const char *p; bool valid; unsigned depth; } Parser;

static void spaces(Parser *p)
{
    while (*p->p == ' ') p->p++;
}

static double sum(Parser *p);

static double atom(Parser *p)
{
    spaces(p);
    if (++p->depth > 24) { p->valid = false; return 0; }
    double n;
    char ch = *p->p;
    if (ch == '(') {
        p->p++;
        n = sum(p);
        spaces(p);
        if (*p->p == ')') p->p++;
        else p->valid = false;
    } else if (ch == '-' || ch == '+') {
        p->p++;
        n = atom(p) * (ch == '-' ? -1 : 1);
    } else if (isdigit((unsigned char)ch) || ch == '.') {
        char *end;
        n = strtod(p->p, &end);
        if (end == p->p) p->valid = false;
        p->p = end;
    } else {
        n = 0;
        p->valid = false;
    }
    p->depth--;
    return n;
}

static double product(Parser *p)
{
    double value = atom(p);
    spaces(p);
    while (p->valid && (*p->p == '*' || *p->p == '/')) {
        char op = *p->p++;
        double factor = atom(p);
        if (op == '*' ) value *= factor;
        else if (factor != 0) value /= factor;
        else p->valid = false;
        spaces(p);
    }
    return value;
}

static double sum(Parser *p)
{
    double value = product(p);
    spaces(p);
    while (p->valid && (*p->p == '+' || *p->p == '-')) {
        char op = *p->p++;
        double term = product(p);
        value += op == '+' ? term : -term;
        spaces(p);
    }
    return value;
}

bool calculate(const char *source, double *out)
{
    Parser p = { .p = source, .valid = true };
    *out = sum(&p);
    spaces(&p);
    return p.valid && !*p.p && isfinite(*out);
}
