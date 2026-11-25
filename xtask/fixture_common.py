"""Shared helpers for xtask fixture builder scripts."""

import hashlib
import os
import stat
import xml.etree.ElementTree as ET

WIX_NS = 'http://schemas.microsoft.com/wix/2006/wi'


def tree(root, *, symlinks=False, executable=False):
    """Walk `root` and return sorted golden-tree entries matching `TreeEntry`.

    When `symlinks` is True, symlinks are recorded with `symlink_target` via
    `lstat`/`readlink` instead of being followed. When `executable` is True,
    file entries reflect POSIX execute bits; otherwise `executable` is always
    `False` (Windows and ISO9660 fixtures).
    """
    entries = []
    for path in sorted(root.rglob('*')):
        name = path.relative_to(root).as_posix()
        if symlinks:
            metadata = path.lstat()
            if stat.S_ISLNK(metadata.st_mode):
                entries.append(
                    dict(
                        path=name,
                        kind='symlink',
                        size=0,
                        executable=False,
                        symlink_target=os.readlink(path),
                    )
                )
                continue
            is_dir = path.is_dir()
            size = 0 if is_dir else metadata.st_size
            is_exec = bool(metadata.st_mode & 0o111) if (executable and not is_dir) else False
        else:
            is_dir = path.is_dir()
            size = 0 if is_dir else path.stat().st_size
            is_exec = bool(path.stat().st_mode & 0o111) if (executable and not is_dir) else False
        if is_dir:
            entries.append(dict(path=name, kind='directory', size=0, executable=False))
        else:
            entries.append(
                dict(
                    path=name,
                    kind='file',
                    size=size,
                    executable=is_exec,
                    sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
                )
            )
    return sorted(entries, key=lambda entry: entry['path'])


def wix_root():
    """Register the WiX namespace and return a fresh `<Wix>` root element."""
    ET.register_namespace('', WIX_NS)
    return ET.Element('{' + WIX_NS + '}Wix')


def wix_node(parent, tag, **attrs):
    """Append a WiX-namespaced child element to `parent`."""
    return ET.SubElement(parent, '{' + WIX_NS + '}' + tag, attrs)


def add_wix_payload_tree(install, feature, payload, component_guid=None):
    """Populate WiX Directory/Component/File/ComponentRef nodes from `payload`."""
    dirs = {'.': install}
    guid_for = component_guid if component_guid is not None else (lambda _rel: '*')
    for index, path in enumerate(sorted(payload.rglob('*'))):
        relative = path.relative_to(payload)
        parent = dirs[relative.parent.as_posix()]
        if path.is_dir():
            dirs[relative.as_posix()] = wix_node(parent, 'Directory', Id=f'D{index}', Name=path.name)
        else:
            component = wix_node(
                parent, 'Component', Id=f'C{index}', Guid=guid_for(relative.as_posix())
            )
            wix_node(component, 'File', Id=f'F{index}', Name=path.name, Source=str(path), KeyPath='yes')
            wix_node(feature, 'ComponentRef', Id=f'C{index}')
