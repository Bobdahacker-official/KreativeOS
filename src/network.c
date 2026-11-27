#include "kos.h"

#include <arpa/inet.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <ifaddrs.h>
#include <net/if.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

#define AGENT_SOCKET "/run/kos-network.sock"
#define WIFI_CONTROL "/run/wpa_supplicant"
#define REPLY_CAP 8192
enum { WIFI_STATUS, WIFI_SCAN, WIFI_RESULTS, WIFI_CONNECT };
typedef struct { int action; char ssid[33], password[65]; bool open; } Request;
typedef struct { char iface[IFNAMSIZ]; int id; time_t until; char state[64]; } Pending;

static bool iface_name(const char *s)
{
    if (!*s || strlen(s) >= IFNAMSIZ) return false;
    for (; *s; s++)
        if (!((*s >= 'a' && *s <= 'z') || (*s >= 'A' && *s <= 'Z') ||
              (*s >= '0' && *s <= '9') || *s == '-' || *s == '_' || *s == '.')) return false;
    return true;
}

static bool supplicant_interface(char iface[IFNAMSIZ])
{
    DIR *dir = opendir(WIFI_CONTROL);
    if (!dir) return false;
    struct dirent *entry;
    bool found = false;
    while ((entry = readdir(dir))) {
        struct stat st;
        if (iface_name(entry->d_name) &&
            !fstatat(dirfd(dir), entry->d_name, &st, AT_SYMLINK_NOFOLLOW) && S_ISSOCK(st.st_mode)) {
            strcpy(iface, entry->d_name);
            found = true;
            break;
        }
    }
    closedir(dir);
    return found;
}

static bool control(const char *iface, const char *command, char *out, size_t cap)
{
    int fd = socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0);
    if (fd < 0) return false;
    static unsigned seq;
    struct sockaddr_un local = { .sun_family = AF_UNIX };
    struct sockaddr_un peer = { .sun_family = AF_UNIX };
    snprintf(local.sun_path, sizeof(local.sun_path), "/run/.kosp-%ld-%u", (long)getpid(), seq++);
    snprintf(peer.sun_path, sizeof(peer.sun_path), "%s/%s", WIFI_CONTROL, iface);
    bool ok = bind(fd, (struct sockaddr *)&local, sizeof(local)) == 0;
    if (ok) {
        struct pollfd readable = { fd, POLLIN, 0 };
        ok = connect(fd, (struct sockaddr *)&peer, sizeof(peer)) == 0 &&
             send(fd, command, strlen(command), 0) == (ssize_t)strlen(command) &&
             poll(&readable, 1, 700) == 1;
        if (ok) {
            ssize_t size = recv(fd, out, cap - 1, 0);
            ok = size >= 0;
            if (ok) out[size] = 0;
        }
        unlink(local.sun_path);
    }
    close(fd);
    return ok;
}

static bool command_ok(const char *iface, const char *command)
{
    char reply[128];
    return control(iface, command, reply, sizeof(reply)) && !strncmp(reply, "OK", 2);
}

static bool wifi_configure(Pending *pending, const char *iface, const Request *request)
{
    if (pending->id >= 0) return false;
    size_t len = strnlen(request->ssid, sizeof(request->ssid));
    size_t pass = strnlen(request->password, sizeof(request->password));
    if (!len || len > 32 || (!request->open && (pass < 8 || pass > 63))) return false;
    for (size_t i = 0; i < pass; i++)
        if ((unsigned char)request->password[i] < 32 || (unsigned char)request->password[i] > 126) return false;
    char reply[160];
    if (!control(iface, "ADD_NETWORK", reply, sizeof(reply))) return false;
    char *end;
    long id = strtol(reply, &end, 10);
    if (end == reply || (*end != '\n' && *end) || id < 0 || id > 100000) return false;
    char ssid[65], secret[130], cmd[220];
    static const char hex[] = "0123456789abcdef";
    for (size_t i = 0; i < len; i++) {
        unsigned char value = (unsigned char)request->ssid[i];
        ssid[i * 2] = hex[value >> 4];
        ssid[i * 2 + 1] = hex[value & 15];
    }
    ssid[len * 2] = 0;
    size_t used = 0;
    for (size_t i = 0; i < pass; i++) {
        char c = request->password[i];
        if (c == '\\' || c == '"') secret[used++] = '\\';
        secret[used++] = c;
    }
    secret[used] = 0;
    snprintf(cmd, sizeof(cmd), "SET_NETWORK %ld ssid %s", id, ssid);
    bool ok = command_ok(iface, cmd);
    snprintf(cmd, sizeof(cmd), "SET_NETWORK %ld key_mgmt %s", id, request->open ? "NONE" : "WPA-PSK");
    ok = ok && command_ok(iface, cmd);
    if (!request->open) {
        snprintf(cmd, sizeof(cmd), "SET_NETWORK %ld psk \"%s\"", id, secret);
        ok = ok && command_ok(iface, cmd);
    }
    explicit_bzero(secret, sizeof(secret));
    explicit_bzero(cmd, sizeof(cmd));
    snprintf(cmd, sizeof(cmd), "SELECT_NETWORK %ld", id);
    ok = ok && command_ok(iface, cmd);
    if (!ok) {
        snprintf(cmd, sizeof(cmd), "REMOVE_NETWORK %ld", id);
        command_ok(iface, cmd);
    } else {
        pending->id = (int)id;
        pending->until = time(NULL) + 60;
        strcpy(pending->iface, iface);
        strcpy(pending->state, "Joining Wi-Fi");
    }
    return ok;
}

static void wifi_watch(Pending *pending)
{
    if (pending->id < 0) return;
    char status[2048], match[40], command[64];
    snprintf(match, sizeof(match), "\nid=%d\n", pending->id);
    bool connected = control(pending->iface, "STATUS", status, sizeof(status)) &&
        strstr(status, "wpa_state=COMPLETED\n") && strstr(status, match);
    if (!connected && time(NULL) < pending->until) return;
    if (!connected) {
        snprintf(command, sizeof(command), "REMOVE_NETWORK %d", pending->id);
        command_ok(pending->iface, command);
        command_ok(pending->iface, "ENABLE_NETWORK all");
        strcpy(pending->state, "Could not join this Wi-Fi network");
    } else {
        bool saved = command_ok(pending->iface, "SAVE_CONFIG");
        strcpy(pending->state, saved ? "Wi-Fi connected and saved" : "Wi-Fi connected for this boot");
    }
    pending->id = -1;
}

int network_agent(void)
{
    if (geteuid()) return 1;
    int server = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0);
    if (server < 0) return 1;
    struct sockaddr_un address = { .sun_family = AF_UNIX, .sun_path = AGENT_SOCKET };
    unlink(AGENT_SOCKET);
    if (bind(server, (struct sockaddr *)&address, sizeof(address)) ||
        chmod(AGENT_SOCKET, 0666) || listen(server, 4)) {
        close(server);
        return 1;
    }
    Pending pending = { .id = -1 };
    for (;;) {
        struct pollfd listening = { server, POLLIN, 0 };
        int active = poll(&listening, 1, pending.id >= 0 ? 900 : -1);
        if (active > 0) {
            int client = accept4(server, NULL, NULL, SOCK_CLOEXEC | SOCK_NONBLOCK);
            if (client >= 0) {
                struct pollfd packet = { client, POLLIN, 0 };
                Request r = {0};
                char reply[REPLY_CAP] = "";
                char iface[IFNAMSIZ];
                if (poll(&packet, 1, 400) == 1 && recv(client, &r, sizeof(r), MSG_TRUNC) == sizeof(r)) {
                    if (!supplicant_interface(iface)) {
                        strcpy(reply, "No Wi-Fi adapter found");
                    } else switch (r.action) {
                    case WIFI_STATUS:
                        if (*pending.state) strcpy(reply, pending.state);
                        else if (control(iface, "STATUS", reply, sizeof(reply)) && strstr(reply, "wpa_state=COMPLETED"))
                            strcpy(reply, "Wi-Fi connected");
                        else strcpy(reply, "Wi-Fi available");
                        break;
                    case WIFI_SCAN:
                        strcpy(reply, command_ok(iface, "SCAN") ? "Scan started" : "Scan in progress or unavailable");
                        if (pending.id < 0) pending.state[0] = 0;
                        break;
                    case WIFI_RESULTS:
                        if (!control(iface, "SCAN_RESULTS", reply, sizeof(reply))) strcpy(reply, "Scan unavailable");
                        break;
                    case WIFI_CONNECT:
                        strcpy(reply, wifi_configure(&pending, iface, &r) ?
                                      "Joining Wi-Fi" : "Could not start the connection");
                        break;
                    default: strcpy(reply, "Invalid request"); break;
                    }
                    send(client, reply, strlen(reply), MSG_NOSIGNAL);
                }
                explicit_bzero(&r, sizeof(r));
                close(client);
            }
        } else if (active < 0 && errno != EINTR) {
            break;
        }
        wifi_watch(&pending);
    }
    close(server);
    unlink(AGENT_SOCKET);
    return 1;
}

static bool request(Request *r, char *out, size_t size)
{
    int fd = socket(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    if (fd < 0) return false;
    struct sockaddr_un agent = { .sun_family = AF_UNIX, .sun_path = AGENT_SOCKET };
    struct pollfd response = { fd, POLLIN, 0 };
    bool ok = connect(fd, (struct sockaddr *)&agent, sizeof(agent)) == 0 &&
              send(fd, r, sizeof(*r), MSG_NOSIGNAL) == sizeof(*r) &&
              poll(&response, 1, 850) == 1;
    if (ok) {
        ssize_t count = recv(fd, out, size - 1, 0);
        ok = count > 0;
        if (ok) out[count] = 0;
    }
    close(fd);
    return ok;
}

bool network_status(State *s)
{
    char updated[sizeof(s->network_status)] = "";
    struct ifaddrs *list;
    bool linked = false;
    if (!getifaddrs(&list)) {
        for (struct ifaddrs *i = list; i; i = i->ifa_next) {
            if (!i->ifa_addr || i->ifa_addr->sa_family != AF_INET ||
                (i->ifa_flags & IFF_LOOPBACK)) continue;
            char addr[INET_ADDRSTRLEN];
            struct sockaddr_in *ip = (struct sockaddr_in *)i->ifa_addr;
            if (inet_ntop(AF_INET, &ip->sin_addr, addr, sizeof(addr))) {
                snprintf(updated, sizeof(updated), "%.20s  %s", i->ifa_name, addr);
                linked = true;
                break;
            }
        }
        freeifaddrs(list);
    }
    if (!linked) strcpy(updated, "No IPv4 connection");
    bool changed = strcmp(updated, s->network_status) != 0;
    strcpy(s->network_status, updated);
    return changed;
}

void network_scan(State *s)
{
    char reply[192];
    Request scan = { .action = WIFI_SCAN };
    if (request(&scan, reply, sizeof(reply))) snprintf(s->message, sizeof(s->message), "%.95s", reply);
    else strcpy(s->message, "Use the host network settings on this installation.");
    s->wifi_retries = 5;
    s->wifi_scan_at = s->now + 5000;
}

static unsigned hex_digit(char c)
{
    if (c >= '0' && c <= '9') return (unsigned)(c - '0');
    if (c >= 'a' && c <= 'f') return (unsigned)(c - 'a' + 10);
    if (c >= 'A' && c <= 'F') return (unsigned)(c - 'A' + 10);
    return 16;
}

static void decode_ssid(char dest[33], const char *text)
{
    int n = 0;
    while (*text && n < 32) {
        unsigned char value = (unsigned char)*text++;
        if (value == '\\' && text[0] == 'x' && text[1] && text[2]) {
            unsigned hi = hex_digit(text[1]), lo = hex_digit(text[2]);
            if (hi < 16 && lo < 16) { value = (unsigned char)(hi * 16 + lo); text += 3; }
        } else if (value == '\\' && *text == '\\') {
            text++;
        }
        if (!value) { dest[0] = 0; return; }
        dest[n++] = (char)value;
    }
    dest[n] = 0;
}

void network_refresh(State *s)
{
    if (!s->wifi_scan_at || (int)(s->now - s->wifi_scan_at) < 0 || s->wifi_edit) return;
    char reply[REPLY_CAP];
    Request results = { .action = WIFI_RESULTS };
    s->wifi_scan_at = 0;
    if (!request(&results, reply, sizeof(reply)) || !strchr(reply, '\n')) return;
    s->wifi_count = 0;
    char *rows;
    char *line = strtok_r(reply, "\n", &rows);
    while (s->wifi_count < 12 && (line = strtok_r(NULL, "\n", &rows))) {
        char *fields;
        char *bssid = strtok_r(line, "\t", &fields);
        char *freq = strtok_r(NULL, "\t", &fields);
        char *signal = strtok_r(NULL, "\t", &fields);
        char *flags = strtok_r(NULL, "\t", &fields);
        if (!bssid || !freq || !signal || !flags || !*fields) continue;
        char decoded[33];
        decode_ssid(decoded, fields);
        if (!*decoded) continue;
        bool duplicate = false;
        for (int i = 0; i < s->wifi_count; i++)
            if (!strcmp(decoded, s->wifi_results[i])) duplicate = true;
        if (duplicate) continue;
        strcpy(s->wifi_results[s->wifi_count], decoded);
        s->wifi_secured[s->wifi_count++] = strstr(flags, "WPA") || strstr(flags, "WEP");
    }
    if (s->wifi_count) {
        s->message[0] = 0;
        s->wifi_retries = 0;
    } else if (s->wifi_retries-- > 0) {
        strcpy(s->message, "Scanning for Wi-Fi networks...");
        s->wifi_scan_at = s->now + 5000;
    } else {
        strcpy(s->message, "No Wi-Fi networks found");
    }
    s->redraw = true;
}

void network_connect(State *s)
{
    if (!s->wifi_open && strlen(s->wifi_password) < 8) {
        strcpy(s->message, "Enter the network password.");
        return;
    }
    char reply[192];
    Request r = { .action = WIFI_CONNECT, .open = s->wifi_open };
    strcpy(r.ssid, s->wifi_ssid);
    strcpy(r.password, s->wifi_password);
    bool sent = request(&r, reply, sizeof(reply));
    explicit_bzero(&r, sizeof(r));
    explicit_bzero(s->wifi_password, sizeof(s->wifi_password));
    if (sent) snprintf(s->message, sizeof(s->message), "%.95s", reply);
    else strcpy(s->message, "Wi-Fi service did not respond.");
    s->wifi_edit = s->keyboard = false;
    s->wifi_join_at = s->now + 2500;
}

void network_join_update(State *s)
{
    if (!s->wifi_join_at || (int)(s->now - s->wifi_join_at) < 0) return;
    Request r = { .action = WIFI_STATUS };
    char reply[192];
    if (request(&r, reply, sizeof(reply))) {
        snprintf(s->message, sizeof(s->message), "%.95s", reply);
        s->wifi_join_at = !strcmp(reply, "Joining Wi-Fi") ? s->now + 2500 : 0;
    } else {
        s->wifi_join_at = 0;
    }
    s->redraw = true;
}
