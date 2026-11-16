"""Stable XRC names for interface text localized through typed Rust bindings."""

def name_translatable_controls(root):
    count = 0
    for obj in root.iter('object'):
        cls = obj.get('class', '')
        if not cls.startswith('wx') or cls.endswith('Sizer'):
            continue
        fields = [obj.findtext(tag) for tag in ('label', 'title', 'tooltip', 'hint')]
        content = obj.find('content') if cls in ('wxComboBox', 'wxListBox') else None
        if not any(fields) and content is None:
            continue
        count += 1
        if not obj.get('name'):
            obj.set('name', f'i18n_control_{count}')
