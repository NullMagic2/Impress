#include <wx/wxprec.h>
#include <wx/wx.h>
#include "wx/filedlg.h"
#include "wx/filedlgcustomize.h"
#include "wx/arrstr.h"
#include "wx/filename.h"
#ifdef __WXGTK3__
#include <gtk/gtk.h>
#endif
#include "../include/wxdragon.h"
#include "wxd_utils.h" // For WXD_STR_TO_WX_STRING_UTF8_NULL_OK and GET_WX_STRING_RESULT

extern void
wxd_ArrayString_AssignFromWxArrayString(wxd_ArrayString_t* target, const wxArrayString& source);

// --- wxFileDialog ---

// Own the hook with the dialog: wxWidgets only borrows it through ShowModal.
// The customize API preserves the modern Windows file picker and uses GTK's
// extra-control area on Linux.
class ImpressFontSaveHook : public wxFileDialogCustomizeHook {
public:
    explicit ImpressFontSaveHook(wxFileDialog* dialog) : dialog(dialog) {}
    bool value = false;
    void AddCustomControls(wxFileDialogCustomize& controls) override {
        checkbox = controls.AddCheckBox(wxGetTranslation("Embed fonts", "impress"));
        checkbox->SetValue(value);
        UpdateCustomControls();
    }
    void UpdateCustomControls() override {
        if (!checkbox) return;
        int filter = dialog->GetCurrentlySelectedFilterIndex();
        if (filter < 0) filter = dialog->GetFilterIndex();
        // Impress Save's first two filters are native Impress and DOCX.
        checkbox->Enable(filter == 0 || filter == 1);
    }
    void TransferDataFromCustomControls() override {
        if (checkbox) value = checkbox->GetValue();
    }
private:
    wxFileDialog* dialog;
    wxFileDialogCheckBox* checkbox = nullptr;
};
class ImpressFileDialog : public wxFileDialog {
public:
    using wxFileDialog::wxFileDialog;
    ImpressFontSaveHook fontHook{this};
#ifdef __WXGTK3__
    // GTK restores the previous chooser size, even on a smaller monitor.
    // wxFileDialog::SetSize is a no-op on GTK. Resize the mapped GTK chooser.
    int ShowModal() override {
        // Windows' native save picker handles this itself. GTK doesn't replace
        // the displayed suffix when the user changes the file-type filter.
        auto* chooser = GTK_FILE_CHOOSER(GetHandle());
        lastSaveFilter = GetFilterIndex();
        gulong filterChanged = 0;
        if (HasFlag(wxFD_SAVE))
            filterChanged = g_signal_connect(chooser, "notify::filter",
                                             G_CALLBACK(SaveFilterChanged), this);
        GSource* fit = g_idle_source_new();
        g_source_set_callback(fit, FitChooser, GetParent() ? GetParent()->GetHandle() : nullptr, nullptr);
        g_source_attach(fit, nullptr);
        int result = wxFileDialog::ShowModal();
        if (filterChanged) g_signal_handler_disconnect(chooser, filterChanged);
        g_source_destroy(fit);
        g_source_unref(fit);
        return result;
    }
private:
    int lastSaveFilter = -1;

    static void SaveFilterChanged(GObject* object, GParamSpec*, gpointer data) {
        auto* dialog = static_cast<ImpressFileDialog*>(data);
        const int filter = dialog->GetFilterIndex();
        if (filter == dialog->lastSaveFilter) return;
        dialog->lastSaveFilter = filter;

        wxArrayString descriptions, patterns;
        wxParseCommonDialogsFilter(dialog->GetWildcard(), descriptions, patterns);
        if (filter < 0 || static_cast<size_t>(filter) >= patterns.size()) return;
        // A multi-pattern or wildcard-only filter has no unambiguous suffix.
        const wxString pattern = patterns[filter];
        if (!pattern.StartsWith("*.") || pattern.find_first_of(";*?/\\", 2) != wxString::npos)
            return;

        auto* chooser = GTK_FILE_CHOOSER(object);
        gchar* currentName = gtk_file_chooser_get_current_name(chooser);
        const wxString name = wxString::FromUTF8(currentName ? currentName : "");
        g_free(currentName);
        if (!name.empty()) {
            wxFileName filename(name);
            const wxString extension = pattern.Mid(2);
            if (filename.GetExt().CmpNoCase(extension) != 0) {
                filename.SetExt(extension);
                const wxString updated = filename.GetFullPath();
                gtk_file_chooser_set_current_name(chooser, updated.utf8_str());
            }
        }
        dialog->fontHook.UpdateCustomControls();
    }

    static gboolean FitChooser(gpointer parent) {
        GList* windows = gtk_window_list_toplevels();
        for (GList* item = windows; item; item = item->next) {
            auto* widget = GTK_WIDGET(item->data);
            if (!GTK_IS_FILE_CHOOSER_DIALOG(widget) || !gtk_widget_get_mapped(widget)) continue;
            auto* window = GTK_WINDOW(widget);
            if (parent != gtk_window_get_transient_for(window)) continue;
            auto* monitor = gdk_display_get_monitor_at_window(gtk_widget_get_display(widget), gtk_widget_get_window(widget));
            if (!monitor) continue;
            GdkRectangle area;
            gdk_monitor_get_workarea(monitor, &area);
            int width, height;
            gtk_window_get_size(window, &width, &height);
            // Leave room for server-side borders and the title bar.
            int max_width = wxMax(1, area.width - 16);
            int max_height = wxMax(1, area.height - 48);
            if (width > max_width || height > max_height)
                gtk_window_resize(window, wxMin(width, max_width), wxMin(height, max_height));
        }
        g_list_free(windows);
        return G_SOURCE_REMOVE;
    }
#endif
};

WXD_EXPORTED int
wxd_FileDialog_EnableFontEmbedding(wxd_FileDialog_t* self, int previous) {
    if (!self) return 0;
    auto* dialog = static_cast<ImpressFileDialog*>(reinterpret_cast<wxFileDialog*>(self));
    dialog->fontHook.value = previous != 0;
    return dialog->SetCustomizeHook(dialog->fontHook) ? 1 : 0;
}
WXD_EXPORTED int
wxd_FileDialog_GetFontEmbedding(const wxd_FileDialog_t* self) {
    if (!self) return 0;
    auto* dialog = static_cast<const ImpressFileDialog*>(reinterpret_cast<const wxFileDialog*>(self));
    return dialog->fontHook.value ? 1 : 0;
}


WXD_EXPORTED wxd_FileDialog_t*
wxd_FileDialog_Create(
    wxd_Window_t* parent, const char* message, const char* defaultDir, const char* defaultFile,
    const char* wildcard, wxd_Style_t style, int x,
    int y, // Note: wxFileDialog doesn't directly take x,y,width,height in constructor
    int width, int height)
{ // These are for wxWindow, but FileDialog often uses default/platform sizing

    wxWindow* parent_wx = (wxWindow*)parent;
    wxFileDialog* dlg =
        new ImpressFileDialog(parent_wx, WXD_STR_TO_WX_STRING_UTF8_NULL_OK(message),
                         WXD_STR_TO_WX_STRING_UTF8_NULL_OK(defaultDir),
                         WXD_STR_TO_WX_STRING_UTF8_NULL_OK(defaultFile),
                         WXD_STR_TO_WX_STRING_UTF8_NULL_OK(wildcard), style,
                         wxd_cpp_utils::to_wx(wxd_Point{x, y}) // Position can be passed
                         // Size is typically not passed or is wxDefaultSize for FileDialog
        );
    // If width/height are not -1 (DEFAULT_SIZE), we could call SetSize, but usually not done for file dialogs.
    // Use OR (not AND) so that specifying just one axis still applies - only skip
    // when BOTH width and height are the -1 "unspecified" sentinel.
    if (width != -1 || height != -1) {
        wxSize size = wxd_cpp_utils::to_wx(wxd_Size{width, height});
        dlg->SetSize(size.GetWidth(), size.GetHeight());
    }
    return (wxd_FileDialog_t*)dlg;
}

// ShowModal is inherited from wxd_Dialog_ShowModal

WXD_EXPORTED int
wxd_FileDialog_GetPath(const wxd_FileDialog_t* self, char* buffer, size_t bufLen)
{
    if (!self)
        return -1;
    wxFileDialog* dlg = (wxFileDialog*)self;
    wxString str = dlg->GetPath();
    return (int)wxd_cpp_utils::copy_wxstring_to_buffer(str, buffer, bufLen);
}

WXD_EXPORTED void
wxd_FileDialog_GetPaths(const wxd_FileDialog_t* self, wxd_ArrayString_t* paths_out)
{
    if (!self || !paths_out)
        return;
    wxFileDialog* dlg = (wxFileDialog*)self;
    wxArrayString wx_paths;
    dlg->GetPaths(wx_paths);
    wxd_ArrayString_AssignFromWxArrayString(paths_out, wx_paths);
}

WXD_EXPORTED int
wxd_FileDialog_GetFilename(const wxd_FileDialog_t* self, char* buffer, size_t bufLen)
{
    if (!self)
        return -1;
    wxFileDialog* dlg = (wxFileDialog*)self;
    wxString str = dlg->GetFilename();
    return (int)wxd_cpp_utils::copy_wxstring_to_buffer(str, buffer, bufLen);
}

WXD_EXPORTED void
wxd_FileDialog_GetFilenames(const wxd_FileDialog_t* self, wxd_ArrayString_t* filenames_out)
{
    if (!self || !filenames_out)
        return;
    wxFileDialog* dlg = (wxFileDialog*)self;
    wxArrayString wx_filenames;
    dlg->GetFilenames(wx_filenames);
    wxd_ArrayString_AssignFromWxArrayString(filenames_out, wx_filenames);
}

WXD_EXPORTED int
wxd_FileDialog_GetDirectory(const wxd_FileDialog_t* self, char* buffer, size_t bufLen)
{
    if (!self)
        return -1;
    wxFileDialog* dlg = (wxFileDialog*)self;
    wxString str = dlg->GetDirectory();
    return (int)wxd_cpp_utils::copy_wxstring_to_buffer(str, buffer, bufLen);
}

WXD_EXPORTED int
wxd_FileDialog_GetFilterIndex(const wxd_FileDialog_t* self)
{
    if (!self)
        return -1; // Or some other error indicator, wxNOT_FOUND maybe?
    wxFileDialog* dlg = (wxFileDialog*)self;
    return dlg->GetFilterIndex();
}

// --- Optional Setters ---
WXD_EXPORTED void
wxd_FileDialog_SetMessage(const wxd_FileDialog_t* self, const char* message)
{
    if (!self)
        return;
    ((wxFileDialog*)self)->SetMessage(WXD_STR_TO_WX_STRING_UTF8_NULL_OK(message));
}

WXD_EXPORTED void
wxd_FileDialog_SetPath(const wxd_FileDialog_t* self, const char* path)
{
    if (!self)
        return;
    ((wxFileDialog*)self)->SetPath(WXD_STR_TO_WX_STRING_UTF8_NULL_OK(path));
}

WXD_EXPORTED void
wxd_FileDialog_SetDirectory(const wxd_FileDialog_t* self, const char* directory)
{
    if (!self)
        return;
    ((wxFileDialog*)self)->SetDirectory(WXD_STR_TO_WX_STRING_UTF8_NULL_OK(directory));
}

WXD_EXPORTED void
wxd_FileDialog_SetFilename(const wxd_FileDialog_t* self, const char* name)
{
    if (!self)
        return;
    ((wxFileDialog*)self)->SetFilename(WXD_STR_TO_WX_STRING_UTF8_NULL_OK(name));
}

WXD_EXPORTED void
wxd_FileDialog_SetWildcard(const wxd_FileDialog_t* self, const char* wildCard)
{
    if (!self)
        return;
    ((wxFileDialog*)self)->SetWildcard(WXD_STR_TO_WX_STRING_UTF8_NULL_OK(wildCard));
}

WXD_EXPORTED void
wxd_FileDialog_SetFilterIndex(const wxd_FileDialog_t* self, int filterIndex)
{
    if (!self)
        return;
    ((wxFileDialog*)self)->SetFilterIndex(filterIndex);
}

WXD_EXPORTED int
wxd_FileDialog_GetMessage(const wxd_FileDialog_t* self, char* buffer, size_t bufLen)
{
    if (!self)
        return -1;
    wxFileDialog* dlg = (wxFileDialog*)self;
    wxString msg = dlg->GetMessage();
    return (int)wxd_cpp_utils::copy_wxstring_to_buffer(msg, buffer, bufLen);
}

WXD_EXPORTED int
wxd_FileDialog_GetWildcard(const wxd_FileDialog_t* self, char* buffer, size_t bufLen)
{
    if (!self)
        return -1;
    wxFileDialog* dlg = (wxFileDialog*)self;
    wxString msg = dlg->GetWildcard();
    return (int)wxd_cpp_utils::copy_wxstring_to_buffer(msg, buffer, bufLen);
}

WXD_EXPORTED int
wxd_FileDialog_GetCurrentlySelectedFilterIndex(const wxd_FileDialog_t* self)
{
    if (!self)
        return -1;
    wxFileDialog* dlg = (wxFileDialog*)self;
#ifdef __WXMSW__
    return dlg->GetCurrentlySelectedFilterIndex();
#else
#ifdef __WXOSX__
    return dlg->GetCurrentlySelectedFilterIndex();
#else
    return -1; // Not implemented on this platform
#endif
#endif
}
