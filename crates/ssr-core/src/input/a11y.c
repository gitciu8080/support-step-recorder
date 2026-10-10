// Small typed AT-SPI adapter. All GTK/AT-SPI work stays on this thread.
#include <atspi/atspi.h>
#include <string.h>
#include <unistd.h>

typedef void (*TextCallback)(void *, const char *, const char *, const char *, const char *);
typedef int (*StopCallback)(void *);
typedef struct { TextCallback text; void *data; } State;

static void changed(AtspiEvent *event, void *data) {
    State *s = data;
    AtspiAccessible *obj = event->source;
    AtspiStateSet *states = atspi_accessible_get_state_set(obj);
    gboolean relevant = states && atspi_state_set_contains(states, ATSPI_STATE_FOCUSED)
        && atspi_state_set_contains(states, ATSPI_STATE_EDITABLE);
    if (states) g_object_unref(states);
    if (!relevant) goto done;
    GError *error = NULL;
    AtspiRole role = atspi_accessible_get_role(obj, &error);
    if (error) { g_error_free(error); goto done; }
    if (role == ATSPI_ROLE_PASSWORD_TEXT) goto done;
    guint pid = atspi_accessible_get_process_id(obj, NULL);
    if (pid == (guint)getpid()) goto done;
    AtspiText *text = atspi_accessible_get_text_iface(obj);
    if (!text) goto done;
    int count = atspi_text_get_character_count(text, &error);
    if (error || count < 0 || count > 12000) {
        if (error) g_error_free(error);
        g_object_unref(text); goto done;
    }
    gchar *value = atspi_text_get_text(text, 0, count, &error);
    g_object_unref(text);
    if (error) { g_error_free(error); goto done; }
    gchar *context = g_strdup_printf("a11y:%u:%p", pid, (void *)obj);
    AtspiAccessible *application = atspi_accessible_get_application(obj, NULL);
    gchar *name = application ? atspi_accessible_get_name(application, NULL) : NULL;
    if (application) g_object_unref(application);
    s->text(s->data, strstr(event->type, "focused") ? "a11y_focus" : "a11y_snapshot",
        context, name ? name : "", value ? value : "");
    g_free(name); g_free(context); g_free(value);
done:
    g_boxed_free(ATSPI_TYPE_EVENT, event);
}

int ssr_a11y_run(TextCallback text, StopCallback stop, void *data) {
    if (atspi_init() != 0) return 1;
    atspi_set_timeout(500, 500);
    State state = { text, data };
    AtspiEventListener *listener = atspi_event_listener_new(changed, &state, NULL);
    GError *error = NULL;
    gboolean a = atspi_event_listener_register(listener, "object:state-changed:focused", &error);
    if (error) { g_error_free(error); error = NULL; }
    gboolean b = atspi_event_listener_register(listener, "object:text-changed", &error);
    if (error) g_error_free(error);
    if (a && b) {
        while (!stop(data)) {
            while (g_main_context_pending(NULL) && !stop(data)) g_main_context_iteration(NULL, FALSE);
            g_usleep(10000);
        }
    }
    // Drain callbacks already delivered before the stop request.
    for (int i = 0; i < 32 && g_main_context_pending(NULL); ++i) g_main_context_iteration(NULL, FALSE);
    atspi_event_listener_deregister(listener, "object:state-changed:focused", NULL);
    atspi_event_listener_deregister(listener, "object:text-changed", NULL);
    g_object_unref(listener);
    atspi_exit();
    return a && b ? 0 : 1;
}
