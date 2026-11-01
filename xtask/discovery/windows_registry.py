"""Original Windows .reg writer for authored public-product-shape fixtures."""
import json


def registry(spec):
    def quote(value):
        return json.dumps(value, ensure_ascii=False)
    lines = ['Windows Registry Editor Version 5.00', '']
    for record in spec['records']:
        lines.append('[' + record['key'] + ']')
        for name, value in sorted(record['values'].items()):
            encoded = ('dword:%08x' % value) if isinstance(value, int) else quote(value)
            lines.append(quote(name) + '=' + encoded)
        lines.append('')
    return ('\n'.join(lines) + '\n').encode('utf-8')
