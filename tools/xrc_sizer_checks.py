#!/usr/bin/env python3
"""Validate the box-sizer alignment contracts used by the Ribbon.

These checks inspect XRC; they do not run wxWidgets. Deliberately distinguish a
sizeritem's flags from a control's own style and from a nested sizer's orient.
"""
import xml.etree.ElementTree as ET

BOX_SIZERS = {'wxBoxSizer', 'wxStaticBoxSizer'}


def center_for_sizer(parent: ET.Element) -> str:
    """Center on the cross-axis of the *containing* box sizer."""
    if parent.get('class') not in BOX_SIZERS:
        raise ValueError('Expected a containing box sizer for centered content')
    orient = parent.findtext('orient', 'wxHORIZONTAL')
    if orient == 'wxHORIZONTAL':
        return 'wxALIGN_CENTER_VERTICAL'
    if orient == 'wxVERTICAL':
        return 'wxALIGN_CENTER_HORIZONTAL'
    raise ValueError(f'Unknown box-sizer orientation: {orient}')


def alignment_errors(root: ET.Element) -> list[str]:
    """Return invalid same-axis, duplicate, or expand/alignment combinations.

    Covers box sizers only. The generic CENTER/CENTRE flag is a cross-axis flag
    in a box sizer; LEFT/TOP are zero-valued defaults, not direction bitmasks.
    """
    problems = []
    for sizer in root.iter('object'):
        if sizer.get('class') not in BOX_SIZERS:
            continue
        orient = sizer.findtext('orient', 'wxHORIZONTAL')
        if orient not in ('wxHORIZONTAL', 'wxVERTICAL'):
            problems.append(f'Unknown box-sizer orientation: {orient}')
            continue
        cross = 'vertical' if orient == 'wxHORIZONTAL' else 'horizontal'
        along = 'horizontal' if orient == 'wxHORIZONTAL' else 'vertical'
        for item in sizer.findall('object'):
            if item.get('class') not in ('sizeritem', 'spacer'):
                continue
            child = item.find('object')
            name = child.get('name', child.get('class', 'unnamed')) if child is not None else 'spacer'
            flags = [f.strip().replace('CENTRE', 'CENTER')
                     for f in item.findtext('flag', '').split('|') if f.strip()]
            align = {'horizontal': [], 'vertical': []}
            for flag in flags:
                if flag in ('wxALIGN_CENTER_HORIZONTAL', 'wxALIGN_RIGHT'):
                    align['horizontal'].append(flag)
                elif flag in ('wxALIGN_CENTER_VERTICAL', 'wxALIGN_BOTTOM'):
                    align['vertical'].append(flag)
                elif flag == 'wxALIGN_CENTER':
                    align[cross].append(flag)
            if align[along]:
                problems.append(f'{name}: {orient} has ineffective {along} alignment {align[along]}')
            if len(align[cross]) > 1:
                problems.append(f'{name}: conflicting {cross} alignment flags {align[cross]}')
            if any(flag in ('wxEXPAND', 'wxGROW') for flag in flags) and align[cross]:
                problems.append(f'{name}: expansion conflicts with {cross} alignment {align[cross]}')
    return problems


def require_valid_alignment(root: ET.Element) -> None:
    errors = alignment_errors(root)
    if errors:
        raise ValueError('Invalid Ribbon XRC alignment:\n' + '\n'.join(errors))
