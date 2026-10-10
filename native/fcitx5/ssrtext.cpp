// Only sends text while a recorder-owned socket exists. Never writes text to disk.
#include <fcitx/addonfactory.h>
#include <fcitx/addoninstance.h>
#include <fcitx/addonmanager.h>
#include <fcitx/inputcontext.h>
#include <fcitx/instance.h>
#include <fcitx/surroundingtext.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

static std::string json(const std::string &s) {
    std::string out = "\"";
    const char *hex = "0123456789abcdef";
    for (unsigned char c : s) {
        if (c == '"' || c == '\\') { out += '\\'; out += c; }
        else if (c < 32) { out += "\\u00"; out += hex[c >> 4]; out += hex[c & 15]; }
        else out += c;
    }
    return out + '"';
}

class SSRText : public fcitx::AddonInstance {
public:
    explicit SSRText(fcitx::Instance *instance) {
        fd_ = socket(AF_UNIX, SOCK_DGRAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0);
        auto watch = [&](fcitx::EventType type, auto callback) {
            watchers_.push_back(instance->watchEvent(type, fcitx::EventWatcherPhase::PostInputMethod, callback));
        };
        watch(fcitx::EventType::InputContextCommitString, [this](fcitx::Event &event) {
            auto &e = static_cast<fcitx::CommitStringEvent &>(event);
            send(e.inputContext(), "commit", e.text());
        });
        watch(fcitx::EventType::InputContextFocusIn, [this](fcitx::Event &event) {
            auto *ic = static_cast<fcitx::InputContextEvent &>(event).inputContext();
            send(ic, "focus", "");
            snapshot(ic);
        });
        watch(fcitx::EventType::InputContextSurroundingTextUpdated, [this](fcitx::Event &event) {
            snapshot(static_cast<fcitx::InputContextEvent &>(event).inputContext());
        });
    }
    ~SSRText() override { if (fd_ >= 0) close(fd_); }
private:
    void snapshot(fcitx::InputContext *ic) {
        if (ic->surroundingText().isValid()) send(ic, "snapshot", ic->surroundingText().text());
    }
    void send(fcitx::InputContext *ic, const char *kind, const std::string &text) {
        if (fd_ < 0 || text.size() > 48000) return;
        // Password / sensitive fields must not leave the input method.
        if (ic->capabilityFlags().testAny(fcitx::CapabilityFlag::PasswordOrSensitive)) {
            kind = "sensitive";
        }
        const char *runtime = std::getenv("XDG_RUNTIME_DIR");
        if (!runtime) return;
        std::string path = std::string(runtime) + "/ssr/text.sock";
        struct stat st{};
        if (lstat(path.c_str(), &st) || !S_ISSOCK(st.st_mode) || st.st_uid != getuid() || (st.st_mode & 077)) return;
        sockaddr_un addr{};
        addr.sun_family = AF_UNIX;
        if (path.size() >= sizeof(addr.sun_path)) return;
        std::memcpy(addr.sun_path, path.c_str(), path.size() + 1);
        // UUID scopes edits to the input context, not just an application.
        std::string id;
        const char *hex = "0123456789abcdef";
        for (unsigned char c : ic->uuid()) { id += hex[c >> 4]; id += hex[c & 15]; }
        std::string packet = "{\"kind\":" + json(kind) + ",\"context\":" + json(id)
            + ",\"app\":" + json(ic->program()) + ",\"text\":"
            + json(std::strcmp(kind, "sensitive") == 0 ? "" : text) + "}";
        sendto(fd_, packet.data(), packet.size(), MSG_DONTWAIT | MSG_NOSIGNAL,
               reinterpret_cast<sockaddr *>(&addr), sizeof(addr));
    }
    int fd_ = -1;
    std::vector<std::unique_ptr<fcitx::HandlerTableEntry<fcitx::EventHandler>>> watchers_;
};

class SSRFactory : public fcitx::AddonFactory {
    fcitx::AddonInstance *create(fcitx::AddonManager *manager) override {
        return new SSRText(manager->instance());
    }
};
FCITX_ADDON_FACTORY(SSRFactory)
