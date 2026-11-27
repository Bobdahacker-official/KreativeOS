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

static void append_output(Terminal *term, const char *data, size_t count)
{
    for (size_t i = 0; i < count; i++) {
        unsigned char c = (unsigned char)data[i];
        if (c != '\n' && c != '\t' && (c < 32 || c == 127)) continue;
        if (term->output_len >= sizeof(term->output) - 1) {
            size_t discard = sizeof(term->output) / 4;
            while (discard < term->output_len && term->output[discard] != '\n') discard++;
            if (discard == term->output_len) discard = sizeof(term->output) / 4;
            else discard++;
            memmove(term->output, term->output + discard, term->output_len - discard);
            term->output_len -= discard;
        }
        term->output[term->output_len++] = c == '\t' ? ' ' : (char)c;
    }
    term->output[term->output_len] = 0;
}

static void output(Terminal *term, const char *data)
{
    append_output(term, data, strlen(data));
}

static void terminal_execute(State *s)
{
    Terminal *t = &s->term;
    char *command = t->line;
    while (*command == ' ') command++;
    if (!*command || t->pid > 0) return;
    if (t->history_size == (int)ARRAY_LEN(t->history)) {
        memmove(t->history[0], t->history[1], sizeof(t->history) - sizeof(t->history[0]));
        t->history_size--;
    }
    strcpy(t->history[t->history_size++], command);
    t->history_pos = t->history_size;
    t->pending[0] = 0;
    output(t, "$ ");
    output(t, command);
    output(t, "\n");
    if (!strcmp(command, "clear")) {
        t->output_len = 0;
        t->output[0] = 0;
    } else if (!strcmp(command, "cd") || !strncmp(command, "cd ", 3)) {
        const char *target = command + 2;
        while (*target == ' ') target++;
        char path[PATH_MAX], resolved[PATH_MAX];
        int length;
        if (!*target || !strcmp(target, "~")) length = snprintf(path, sizeof(path), "%s", s->data);
        else if (*target == '/') length = snprintf(path, sizeof(path), "%s", target);
        else length = snprintf(path, sizeof(path), "%s/%s", t->cwd, target);
        struct stat st;
        if (length >= 0 && (size_t)length < sizeof(path) && realpath(path, resolved) &&
            stat(resolved, &st) == 0 && S_ISDIR(st.st_mode))
            strcpy(t->cwd, resolved);
        else
            output(t, "Could not enter directory\n");
    } else {
        int stream[2];
        if (pipe2(stream, O_CLOEXEC) != 0) {
            output(t, "Could not open command output\n");
        } else {
            int flags = fcntl(stream[0], F_GETFL);
            if (flags < 0 || fcntl(stream[0], F_SETFL, flags | O_NONBLOCK) < 0) {
                close(stream[0]);
                close(stream[1]);
                output(t, "Could not configure command output\n");
                goto done;
            }
            pid_t child = fork();
            if (child == 0) {
                setpgid(0, 0);
                close(stream[0]);
                dup2(stream[1], STDOUT_FILENO);
                dup2(stream[1], STDERR_FILENO);
                close(stream[1]);
                int empty = open("/dev/null", O_RDONLY);
                if (empty < 0 || dup2(empty, STDIN_FILENO) < 0) _exit(126);
                if (empty != STDIN_FILENO) close(empty);
                if (geteuid() == 0 && (setgroups(0, NULL) || setgid(65534) || setuid(65534))) _exit(126);
                if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) < 0 || chdir(t->cwd) < 0) {
                    dprintf(STDERR_FILENO, "Cannot access working directory\n");
                    _exit(126);
                }
                char home[PATH_MAX + 5];
                snprintf(home, sizeof(home), "HOME=%s", s->data);
                char *environment[] = { home, "PATH=/bin:/usr/bin:/sbin:/usr/sbin", "TERM=dumb", "LC_ALL=C", NULL };
                char *args[] = { "sh", "-c", command, NULL };
                execve("/bin/sh", args, environment);
                _exit(127);
            }
            close(stream[1]);
            if (child < 0) {
                close(stream[0]);
                output(t, "Could not start command\n");
            } else {
                setpgid(child, child);
                t->pid = child;
                t->fd = stream[0];
                t->stream_done = t->exited = false;
            }
        }
    }
done:
    t->used = t->cursor = 0;
    t->line[0] = 0;
    s->scroll = 0;
}

void terminal_poll(State *s)
{
    Terminal *t = &s->term;
    if (t->pid <= 0) return;
    if (!t->stream_done) {
        char data[2048];
        for (int i = 0; i < 8; i++) {
            ssize_t count = read(t->fd, data, sizeof(data));
            if (count > 0) {
                append_output(t, data, (size_t)count);
                if (s->app == TERMINAL) s->redraw = true;
            } else {
                if (count == 0 || (errno != EAGAIN && errno != EINTR)) {
                    close(t->fd);
                    t->fd = -1;
                    t->stream_done = true;
                }
                break;
            }
        }
    }
    if (!t->exited) t->exited = waitpid(t->pid, &t->status, WNOHANG) == t->pid;
    if (t->exited && t->stream_done) {
        char status[48];
        if (WIFSIGNALED(t->status)) {
            snprintf(status, sizeof(status), "\nStopped by signal %d\n", WTERMSIG(t->status));
            output(t, status);
        } else if (WIFEXITED(t->status) && WEXITSTATUS(t->status)) {
            snprintf(status, sizeof(status), "\nExited with status %d\n", WEXITSTATUS(t->status));
            output(t, status);
        }
        t->pid = 0;
        s->redraw = true;
    }
}

void terminal_key(State *s, int key)
{
    Terminal *t = &s->term;
    if (key == KEY_BREAK_UI && t->pid > 0) {
        kill(-t->pid, SIGKILL);
        return;
    }
    if (key == '\n') {
        terminal_execute(s);
    } else if (key == KEY_UP_UI || key == KEY_DOWN_UI) {
        if (t->history_pos == t->history_size) strcpy(t->pending, t->line);
        if (key == KEY_UP_UI && t->history_pos > 0) t->history_pos--;
        if (key == KEY_DOWN_UI && t->history_pos < t->history_size) t->history_pos++;
        strcpy(t->line, t->history_pos == t->history_size ? t->pending : t->history[t->history_pos]);
        t->cursor = t->used = strlen(t->line);
    } else if (key == KEY_LEFT_UI && t->cursor > 0) {
        t->cursor--;
    } else if (key == KEY_RIGHT_UI && t->cursor < t->used) {
        t->cursor++;
    } else if ((key == '\b' && t->cursor > 0) || (key == KEY_DELETE_UI && t->cursor < t->used)) {
        if (key == KEY_DELETE_UI) t->cursor++;
        memmove(t->line + t->cursor - 1, t->line + t->cursor, t->used - t->cursor + 1);
        t->cursor--;
        t->used--;
    } else if (key >= 32 && key <= 126 && t->used < sizeof(t->line) - 1) {
        memmove(t->line + t->cursor + 1, t->line + t->cursor, t->used - t->cursor + 1);
        t->line[t->cursor++] = (char)key;
        t->used++;
    }
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
