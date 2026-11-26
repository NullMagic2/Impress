// Impress shared nonmodal swatches. Callbacks and windows stay on the UI thread.
#include <wx/wx.h>
#include <wx/popupwin.h>
#include <wx/dcbuffer.h>
#include <wx/colordlg.h>
#include <wx/bmpcbox.h>
#include <wx/weakref.h>
#include <wx/eventfilter.h>
#include <wx/artprov.h>
#include <wx/statline.h>
#include <wx/dcmemory.h>
#include <wx/tooltip.h>
#include <array>
#include <vector>
#include <unordered_map>
#include "../include/wxdragon.h"
#ifdef __WXGTK3__
#include <gtk/gtk.h>
#endif
namespace {
using Callback = int (*)(void*, int, const char*);
constexpr int Apply=0, Add=1, Remove=2, Closed=3, Open=4;
const char* standard[] = {
 "#000000","#404040","#808080","#C0C0C0","#FFFFFF","#203864","#2F5597","#4472C4",
 "#008C88","#548235","#A9D18E","#FFFF00","#BF9000","#ED7D31","#FF0000","#C00000",
 "#7030A0","#FFC0DD","#9EE6F5","#BDD7EE","#B6DFD7","#D5E8C1","#F8CBAD","#D8C5E7"
};
std::vector<wxString> lines(const char* input) {
 std::vector<wxString> out;
 for(const auto& line:wxSplit(wxString::FromUTF8(input?input:""),'\n')) if(!line.empty())out.push_back(line);
 return out;
}
void noMouseFocus(wxWindow* window) {
#ifdef __WXGTK3__
 gtk_widget_set_can_focus(window->GetHandle(),false);
#endif
}
wxColour edge(){return wxSystemSettings::GetColour(wxSYS_COLOUR_BTNSHADOW);}
wxColour accent(){return wxSystemSettings::GetColour(wxSYS_COLOUR_HIGHLIGHT);}
wxColour muted(){auto f=wxSystemSettings::GetColour(wxSYS_COLOUR_WINDOWTEXT),b=wxSystemSettings::GetColour(wxSYS_COLOUR_WINDOW);return wxColour((f.Red()*3+b.Red()*2)/5,(f.Green()*3+b.Green()*2)/5,(f.Blue()*3+b.Blue()*2)/5);}
wxBitmap automaticChip(wxWindow* window,bool empty){
 wxBitmap bitmap(window->FromDIP(23),window->FromDIP(16));wxMemoryDC dc(bitmap);dc.SetBackground(wxBrush(empty?*wxWHITE:*wxBLACK));dc.Clear();dc.SetBrush(*wxTRANSPARENT_BRUSH);dc.SetPen(wxPen(edge()));dc.DrawRectangle(0,0,bitmap.GetWidth(),bitmap.GetHeight());
 if(empty){dc.SetPen(wxPen(wxColour(195,45,35),window->FromDIP(2)));dc.DrawLine(1,bitmap.GetHeight()-2,bitmap.GetWidth()-2,1);}dc.SelectObject(wxNullBitmap);return bitmap;
}
class Chip:public wxControl {
public:
 wxString color;bool selected=false,keyboard=false,hovered=false,empty=false,automatic=false;
 std::function<void()> activate;
 Chip(wxWindow* parent,const wxString& name,int side):wxControl(parent,wxID_ANY,wxDefaultPosition,parent->FromDIP(wxSize(side,side)),wxBORDER_NONE){
  SetMinSize(GetSize());SetName(name);SetToolTip(name);SetBackgroundStyle(wxBG_STYLE_PAINT);noMouseFocus(this);
  Bind(wxEVT_PAINT,[this](wxPaintEvent&){Paint();});
  Bind(wxEVT_LEFT_UP,[this](wxMouseEvent&){if(!empty&&activate)activate();});
  Bind(wxEVT_ENTER_WINDOW,[this](wxMouseEvent&){hovered=true;Refresh();});
  Bind(wxEVT_LEAVE_WINDOW,[this](wxMouseEvent&){hovered=false;Refresh();});
 }
 bool AcceptsFocus()const override{return false;}
private:
 void Paint(){
  wxAutoBufferedPaintDC dc(this);dc.SetBackground(wxBrush(GetParent()->GetBackgroundColour()));dc.Clear();
  wxRect r=GetClientRect();r.Deflate(FromDIP(3));
  if(empty){dc.SetPen(wxPen(edge(),FromDIP(1),wxPENSTYLE_SHORT_DASH));dc.SetBrush(*wxTRANSPARENT_BRUSH);dc.DrawRoundedRectangle(r,FromDIP(3));return;}
  wxColour fill(color.empty()?"#000000":color);dc.SetPen(wxPen(edge(),FromDIP(1)));dc.SetBrush(wxBrush(fill));dc.DrawRoundedRectangle(r,FromDIP(3));
  if(automatic){dc.SetPen(wxPen(wxColour(195,45,35),FromDIP(2)));dc.DrawLine(r.GetBottomLeft(),r.GetTopRight());}
  if(selected||hovered||keyboard){wxRect outer=GetClientRect();outer.Deflate(FromDIP(1));dc.SetPen(wxPen(selected||keyboard?accent():edge(),FromDIP(2)));dc.SetBrush(*wxTRANSPARENT_BRUSH);dc.DrawRoundedRectangle(outer,FromDIP(4));}
  if(selected&&!automatic){int brightness=fill.Red()*299+fill.Green()*587+fill.Blue()*114;dc.SetPen(wxPen(brightness>155000?wxColour(34,34,34):*wxWHITE,FromDIP(2)));wxPoint c(r.x+r.width/2,r.y+r.height/2);dc.DrawLine(c.x-FromDIP(5),c.y,c.x-FromDIP(1),c.y+FromDIP(4));dc.DrawLine(c.x-FromDIP(1),c.y+FromDIP(4),c.x+FromDIP(6),c.y-FromDIP(4));}
 }
};
class Palette;
Palette* activePalette=nullptr;
class Palette final:public wxPopupTransientWindow,public wxEventFilter {
 wxWeakRef<wxWindow> owner,previousFocus;Callback callback;void* context;
 std::vector<wxString> labels,custom,names;std::array<Chip*,40> chips{};
 wxButton *automatic,*add,*remove;wxStaticText *count,*value;Chip* current;
 wxString selected;int customIndex=-1,keyboardIndex=-1;bool noColor,choosing=false,closing=false;
public:
 Palette(wxWindow* parent,const wxPoint& point,const char* initial,bool empty,const char* customs,const char* strings,const char* standardNames,const wxBitmapBundle& addIcon,const wxBitmapBundle& addDisabled,const wxBitmapBundle& deleteIcon,const wxBitmapBundle& deleteDisabled,Callback fn,void* data)
 :wxPopupTransientWindow(parent,wxBORDER_NONE|wxPU_CONTAINS_CONTROLS|wxFRAME_SHAPED),owner(parent),previousFocus(wxWindow::FindFocus()),callback(fn),context(data),labels(lines(strings)),custom(lines(customs)),names(lines(standardNames)),selected(wxString::FromUTF8(initial?initial:"")),noColor(empty){
  custom.resize(std::min(custom.size(),size_t(16)));SetName(Label(5));SetBackgroundStyle(wxBG_STYLE_PAINT);SetBackgroundColour(wxSystemSettings::GetColour(wxSYS_COLOUR_WINDOW));
  auto* panel=new wxPanel(this);panel->SetBackgroundColour(GetBackgroundColour());auto* root=new wxBoxSizer(wxVERTICAL);
  automatic=new wxButton(panel,wxID_ANY,Label(0),wxDefaultPosition,wxDefaultSize,wxBU_LEFT|wxBORDER_NONE);noMouseFocus(automatic);automatic->SetMinSize(FromDIP(wxSize(-1,43)));automatic->SetBitmap(automaticChip(this,noColor));
  automatic->Bind(wxEVT_BUTTON,[this](wxCommandEvent&){Select("",-1);});root->Add(automatic,0,wxEXPAND|wxLEFT|wxRIGHT,FromDIP(10));root->Add(new wxStaticLine(panel),0,wxEXPAND);
  auto* body=new wxBoxSizer(wxVERTICAL);body->Add(Heading(panel,Label(1)),0,wxBOTTOM,FromDIP(6));auto* grid=new wxGridSizer(3,8,FromDIP(2),FromDIP(2));
  for(int i=0;i<24;i++){auto* chip=new Chip(panel,i<int(names.size())?names[i]:wxString(standard[i]),36);chip->color=standard[i];chip->activate=[this,i]{keyboardIndex=i;Select(standard[i],-1);};chips[i]=chip;grid->Add(chip,0,wxEXPAND);}body->Add(grid,0,wxEXPAND);
  auto* header=new wxBoxSizer(wxHORIZONTAL);header->Add(Heading(panel,Label(2)),1,wxALIGN_CENTER_VERTICAL);count=new wxStaticText(panel,wxID_ANY,"");count->SetForegroundColour(muted());header->Add(count,0,wxALIGN_CENTER_VERTICAL);body->Add(header,0,wxEXPAND|wxTOP|wxBOTTOM,FromDIP(12));
  auto* customsGrid=new wxGridSizer(2,8,FromDIP(2),FromDIP(2));for(int i=0;i<16;i++){auto* chip=new Chip(panel,"",37);chip->activate=[this,i]{if(i<int(custom.size())){keyboardIndex=24+i;Select(custom[i],i);}};chips[24+i]=chip;customsGrid->Add(chip,0,wxEXPAND);}body->Add(customsGrid,0,wxEXPAND);
  auto* actions=new wxBoxSizer(wxHORIZONTAL);add=new wxButton(panel,wxID_ANY,Label(3));remove=new wxButton(panel,wxID_ANY,Label(4));for(auto* b:{add,remove}){noMouseFocus(b);b->SetMinSize(FromDIP(wxSize(92,34)));}
  add->SetBitmap(addIcon);add->SetBitmapDisabled(addDisabled);
  remove->SetBitmap(deleteIcon);remove->SetBitmapDisabled(deleteDisabled);
  for(auto* b:{add,remove}){
   b->SetBitmapMargins(FromDIP(6),0);
   b->SetMinSize(wxSize(std::max(FromDIP(92),b->GetBestSize().x+FromDIP(12)),FromDIP(34)));
  }
  add->Bind(wxEVT_BUTTON,[this](wxCommandEvent&){Choose();});remove->Bind(wxEVT_BUTTON,[this](wxCommandEvent&){Delete();});actions->Add(add,0,wxRIGHT,FromDIP(9));actions->Add(remove);body->Add(actions,0,wxTOP,FromDIP(12));root->Add(body,0,wxALL,FromDIP(16));root->Add(new wxStaticLine(panel),0,wxEXPAND);
  auto* footer=new wxBoxSizer(wxHORIZONTAL);current=new Chip(panel,"",23);value=new wxStaticText(panel,wxID_ANY,"");value->SetForegroundColour(muted());footer->Add(current,0,wxALIGN_CENTER_VERTICAL|wxRIGHT,FromDIP(6));footer->Add(value,1,wxALIGN_CENTER_VERTICAL);
  root->Add(footer,0,wxEXPAND|wxALL,FromDIP(10));
  panel->SetSizerAndFit(root);auto* outer=new wxBoxSizer(wxVERTICAL);outer->Add(panel,1,wxEXPAND|wxALL,FromDIP(1));SetSizerAndFit(outer);
  Bind(wxEVT_PAINT,[this](wxPaintEvent&){wxPaintDC dc(this);dc.SetBrush(wxBrush(GetBackgroundColour()));dc.SetPen(wxPen(edge()));auto r=GetClientRect();r.Deflate(0,0);dc.DrawRoundedRectangle(r,FromDIP(8));});
  wxBitmap mask(GetSize().x,GetSize().y,1);wxMemoryDC shape(mask);shape.SetBackground(*wxBLACK_BRUSH);shape.Clear();shape.SetPen(*wxWHITE_PEN);shape.SetBrush(*wxWHITE_BRUSH);shape.DrawRoundedRectangle(0,0,GetSize().x,GetSize().y,FromDIP(8));shape.SelectObject(wxNullBitmap);SetShape(wxRegion(mask,*wxBLACK));
  wxToolTip::Enable(false);wxToolTip::Enable(true);
  for(int i=0;i<int(custom.size());i++)if(custom[i].CmpNoCase(selected)==0){customIndex=i;break;}
  Update();Position(point,wxSize(0,0));wxEvtHandler::AddFilter(this);activePalette=this;Popup(previousFocus);
 }
 ~Palette()override{if(activePalette==this)activePalette=nullptr;wxEvtHandler::RemoveFilter(this);if(callback)callback(context,Closed,"");}
 int FilterEvent(wxEvent& event)override{
  if(choosing||closing||!IsShown())return Event_Skip;
  if(event.GetEventType()==wxEVT_CHAR)return Event_Processed;
  if(event.GetEventType()!=wxEVT_KEY_DOWN)return Event_Skip;
  auto& key=static_cast<wxKeyEvent&>(event);return HandleKey(key.GetKeyCode(),key.ShiftDown());
 }
 bool NativeKey(void* raw){
#ifdef __WXGTK3__
  if(choosing||closing||!IsShown())return false;
  auto* event=static_cast<GdkEvent*>(raw);if(event->type!=GDK_KEY_PRESS&&event->type!=GDK_KEY_RELEASE)return false;
  if(event->type==GDK_KEY_RELEASE)return true;
  auto& key=event->key;int code=0;
  switch(key.keyval){
   case GDK_KEY_Escape:code=WXK_ESCAPE;break;case GDK_KEY_Return:case GDK_KEY_KP_Enter:code=WXK_RETURN;break;
   case GDK_KEY_space:code=WXK_SPACE;break;case GDK_KEY_Delete:code=WXK_DELETE;break;
   case GDK_KEY_Left:code=WXK_LEFT;break;case GDK_KEY_Right:code=WXK_RIGHT;break;
   case GDK_KEY_Up:code=WXK_UP;break;case GDK_KEY_Down:code=WXK_DOWN;break;
   case GDK_KEY_Tab:case GDK_KEY_ISO_Left_Tab:code=WXK_TAB;break;case GDK_KEY_Home:code=WXK_HOME;break;case GDK_KEY_End:code=WXK_END;break;
  }
  // Application shortcuts dismiss the picker and continue normally.
  if((key.state&(GDK_CONTROL_MASK|GDK_MOD1_MASK))&&key.keyval!=GDK_KEY_Control_L&&key.keyval!=GDK_KEY_Control_R&&key.keyval!=GDK_KEY_Alt_L&&key.keyval!=GDK_KEY_Alt_R){ClosePalette();return false;}
  HandleKey(code,(key.state&GDK_SHIFT_MASK)!=0);return true;
#else
  return false;
#endif
 }
 int HandleKey(int code,bool shift){
  if(code==WXK_ESCAPE){ClosePalette();return Event_Processed;}if(code==WXK_RETURN||code==WXK_NUMPAD_ENTER||code==WXK_SPACE){Activate();return Event_Processed;}if(code==WXK_DELETE){Delete();return Event_Processed;}
  int step=0;if(code==WXK_LEFT)step=-1;else if(code==WXK_RIGHT)step=1;else if(code==WXK_UP)step=-8;else if(code==WXK_DOWN)step=8;else if(code==WXK_TAB)step=shift?-1:1;else if(code==WXK_HOME){keyboardIndex=-1;Update();return Event_Processed;}else if(code==WXK_END){keyboardIndex=41;while(!Available(keyboardIndex))--keyboardIndex;Update();return Event_Processed;}
  if(step){int candidate=keyboardIndex;for(int i=0;i<43;i++){candidate+=step;if(candidate>41)candidate=-1;if(candidate< -1)candidate=41;if(Available(candidate)){keyboardIndex=candidate;break;}}Update();}return Event_Processed;
 }
protected:
 void OnDismiss()override{if(!choosing)ClosePalette();}
private:
 wxString Label(size_t i){return i<labels.size()?labels[i]:wxString();}
 wxStaticText* Heading(wxWindow* p,const wxString& text){auto* l=new wxStaticText(p,wxID_ANY,text);auto font=l->GetFont();font.SetWeight(wxFONTWEIGHT_BOLD);l->SetFont(font);l->SetForegroundColour(muted());return l;}
 bool Emit(int action,const wxString& color){auto utf8=color.ToUTF8();return callback&&callback(context,action,utf8.data())!=0;}
 bool Available(int i){return i<24||(i<40&&i-24<int(custom.size()))||(i==40&&add->IsEnabled())||(i==41&&remove->IsEnabled());}
 void Activate(){if(keyboardIndex<0)Select("",-1);else if(keyboardIndex<24)Select(standard[keyboardIndex],-1);else if(keyboardIndex<40&&keyboardIndex-24<int(custom.size()))Select(custom[keyboardIndex-24],keyboardIndex-24);else if(keyboardIndex==40)Choose();else if(keyboardIndex==41)Delete();}
 void Update(){
  for(int i=0;i<40;i++){auto* c=chips[i];c->empty=i>=24&&i-24>=int(custom.size());if(i>=24&&!c->empty){c->color=custom[i-24];c->SetName(c->color);c->SetToolTip(c->color);}c->selected=!c->empty&&!selected.empty()&&c->color.CmpNoCase(selected)==0&&(i<24?customIndex<0:customIndex==i-24);c->keyboard=keyboardIndex==i;c->Refresh();}
  count->SetLabel(wxString::Format("%zu / 16",custom.size()));add->Enable(custom.size()<16);remove->Enable(customIndex>=0&&customIndex<int(custom.size())&&custom[customIndex]==selected);current->color=selected;current->automatic=selected.empty()&&noColor;current->Refresh();value->SetLabel(selected.empty()?Label(0):selected);automatic->SetLabel(Label(0)+(selected.empty()?wxString::FromUTF8("    ✓"):wxString()));Layout();
 }
 void Select(const wxString& color,int index){if(Emit(Apply,color)){selected=color;customIndex=index;Update();}}
 void Delete(){if(customIndex<0||customIndex>=int(custom.size()))return;if(Emit(Remove,custom[customIndex])){custom.erase(custom.begin()+customIndex);customIndex=-1;Update();}}
 void Choose(){
  if(custom.size()>=16)return;choosing=true;Dismiss();wxColourData data;data.SetChooseFull(true);data.SetColour(wxColour(selected.empty()?"#3D72B4":selected));for(size_t i=0;i<custom.size();i++)data.SetCustomColour(i,wxColour(custom[i]));
  wxColourDialog dialog(owner,&data);dialog.SetTitle(Label(5));bool accepted=dialog.ShowModal()==wxID_OK;
  if(accepted){
   wxString color=dialog.GetColourData().GetColour().GetAsString(wxC2S_HTML_SYNTAX).Upper();int index=-1;
   for(int i=0;i<int(custom.size());i++)if(custom[i]==color){index=i;break;}
   if(index<0&&Emit(Add,color)){index=int(custom.size());custom.push_back(color);}
   if(index>=0)Select(color,index);
  }
  choosing=false;Update();Popup(previousFocus);
 }
 void ClosePalette(){if(closing)return;closing=true;Dismiss();Destroy();}
};
struct ComboProxy {
 wxBitmapComboBox* combo;wxWeakRef<wxButton> button;Callback callback;void* context;
 void Refresh(){if(!button)return;auto selection=combo->GetSelection();button->SetLabel(combo->GetValue()+wxString::FromUTF8("  ⌄"));if(selection!=wxNOT_FOUND)button->SetBitmap(combo->GetItemBitmap(selection));button->Enable(combo->IsEnabled());}
};
std::unordered_map<wxWindow*,ComboProxy*> proxies;
}
extern "C" {
WXD_EXPORTED bool wxd_ColourPalette_FilterNative(void* event){return activePalette&&activePalette->NativeKey(event);}
WXD_EXPORTED void wxd_ColourPalette_Show(void* parent,int x,int y,const char* selected,int empty,const char* custom,const char* labels,const char* names,const void* addIcon,const void* addDisabled,const void* deleteIcon,const void* deleteDisabled,Callback callback,void* context){if(!parent){if(callback)callback(context,Closed,"");return;}auto bitmap=[](const void* raw){return raw?*static_cast<const wxBitmapBundle*>(raw):wxBitmapBundle();};new Palette(static_cast<wxWindow*>(parent),wxPoint(x,y),selected,empty!=0,custom,labels,names,bitmap(addIcon),bitmap(addDisabled),bitmap(deleteIcon),bitmap(deleteDisabled),callback,context);}
WXD_EXPORTED void wxd_ColourPalette_ComboAnchor(void* raw,int* x,int* y){
 auto it=proxies.find(static_cast<wxWindow*>(raw));auto* w=it!=proxies.end()&&it->second->button?static_cast<wxWindow*>(it->second->button.get()):static_cast<wxWindow*>(raw);
 auto p=w->ClientToScreen(wxPoint(0,w->GetSize().y));*x=p.x;*y=p.y;
}
WXD_EXPORTED void wxd_ColourPalette_RefreshCombo(void* raw){auto it=proxies.find(static_cast<wxWindow*>(raw));if(it!=proxies.end())it->second->Refresh();}
WXD_EXPORTED void wxd_ColourPalette_BindCombo(void* raw,Callback callback,void* context){
 auto* combo=static_cast<wxBitmapComboBox*>(raw);if(!combo){if(callback)callback(context,Closed,"");return;}auto it=proxies.find(combo);if(it!=proxies.end()){auto* p=it->second;if(p->callback)p->callback(p->context,Closed,"");p->callback=callback;p->context=context;p->Refresh();return;}
 auto* button=new wxButton(combo->GetParent(),wxID_ANY,"",wxDefaultPosition,combo->GetSize(),wxBU_LEFT);button->SetMinSize(combo->GetMinSize());auto* p=new ComboProxy{combo,button,callback,context};proxies[combo]=p;
 if(auto* sizer=combo->GetContainingSizer())sizer->Replace(combo,button);else button->SetPosition(combo->GetPosition());combo->Hide();p->Refresh();
 button->Bind(wxEVT_BUTTON,[p](wxCommandEvent&){if(p->callback)p->callback(p->context,Open,"");});
 button->Bind(wxEVT_KEY_DOWN,[p](wxKeyEvent& e){if(e.AltDown()&&e.GetKeyCode()==WXK_DOWN){if(p->callback)p->callback(p->context,Open,"");}else e.Skip();});
 combo->Bind(wxEVT_DESTROY,[combo,p](wxWindowDestroyEvent& e){if(e.GetEventObject()==combo){if(p->callback)p->callback(p->context,Closed,"");proxies.erase(combo);if(p->button)p->button->Destroy();delete p;}e.Skip();});combo->GetParent()->Layout();
}
}
