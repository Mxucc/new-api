"""Keep the fork's classic-frontend support in controller/option.go.

The fork accepts the ``classic`` frontend theme; upstream only accepts
``default``. Upstream is authoritative for the rest of the ``theme.frontend``
case, so the validation branch is rewritten in place. Used both by the sync
workflow and by its deterministic retry.
"""

import re
from pathlib import Path

path = Path('controller/option.go')
source = path.read_text()
block = re.compile(r'(?ms)^\tcase "theme\.frontend":\n.*?(?=^\tcase "GroupRatio":)')
match = block.search(source)
if match is None:
    raise SystemExit('controller/option.go no longer contains the frontend theme validation block')

current = match.group(0)
if 'option.Value != "default" && option.Value != "classic"' not in current:
    replacement = (
        '\tcase "theme.frontend":\n'
        '\t\tif option.Value != "default" && option.Value != "classic" {\n'
        '\t\t\tc.JSON(http.StatusOK, gin.H{\n'
        '\t\t\t\t"success": false,\n'
        '\t\t\t\t"message": "无效的主题值，可选值：default（新版前端）、classic（经典前端）",\n'
        '\t\t\t})\n'
        '\t\t\treturn\n'
        '\t\t}\n'
    )
    path.write_text(source[:match.start()] + replacement + source[match.end():])
    print('Restored the classic frontend theme validation in controller/option.go')
else:
    print('controller/option.go already accepts the classic frontend theme')
