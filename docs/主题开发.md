# 羽迹探针主题开发文档

适用版本：主控与界面 0.9.0 起。主题格式版本：1。

## 制作第一个主题

1. 下载后台「主题」页面中的「示例主题」，或复制仓库 `docs/themes/waterside.yuji-theme.json`。
2. 用文本编辑器修改主题名称、作者、版本和配色，保存为 UTF-8 的 `你的主题.yuji-theme.json`。
3. 在自己的面板登录管理员，打开「主题 → 上传主题包」，选择文件。服务端校验通过后进入全站预览。
4. 检查看板、节点详情、管理员后台、SSH 终端以及浅色／深色模式；满意后点击「保存主题」。预览不会改变其他访客的外观。
5. 点击「导出已保存主题」即可下载可分享的主题包。导出的只有外观配置，不含账户、SSH 信息、节点、密钥或站点数据库。

主题由站点管理员安装，公开访客使用站点已经选定的主题。每个站点当前启用一个主题；可保存不同主题文件，需要时重新上传。主题不需要编译，不需要在服务器解压或执行命令。

## 文件结构

单个 JSON 文件最大 **3 MiB**，文件名以 `.json` 结尾，推荐 `.yuji-theme.json`。顶层严格包含以下字段：

```json
{
  "format": "yuji-theme",
  "schemaVersion": 1,
  "metadata": {
    "id": "my-theme",
    "name": "我的主题",
    "author": "作者名称",
    "version": "1.0.0",
    "description": "主题简介",
    "license": "MIT"
  },
  "theme": {
    "preset": "clear",
    "accent": "",
    "background": null,
    "favicon": null,
    "backgroundDim": 20,
    "backgroundBlur": 0,
    "brandIcon": true,
    "palette": {
      "light": { "accent": "#2f7162", "text": "#233c35" },
      "dark": { "accent": "#b3dbc2", "text": "#e2eee5" }
    },
    "customCss": ".server-card { border-radius: 20px; }"
  }
}
```

`format` 必须为 `yuji-theme`，`schemaVersion` 必须为数字 `1`。不支持的字段会被拒绝，避免拼写错误静默失效。`revision` 与 `package` 属于面板运行字段；主题文件应省略，导出功能会自动移除。

### 主题信息

| 字段 | 约束 |
| --- | --- |
| id | 必填，1–48 个 ASCII 字符，小写字母开头，其余只用小写字母、数字和连字符 |
| name | 必填，最多 48 个字符 |
| author | 最多 80 个字符，可留空 |
| version | 必填，最多 32 个 ASCII 字符，只用字母、数字、`.`、`+`、`-`，推荐 `1.0.0` |
| description | 最多 400 个字符，可留空 |
| license | 最多 80 字节，可填 `MIT`、`CC0-1.0` 或素材许可说明 |

名称、作者、简介和许可使用普通文本，不能包含控制字符。界面会把它们作为文字显示。

## 选择基础外观

`theme.preset` 选用以下一种基础布局，再由配色和局部样式进行修改：

| 值 | 外观 |
| --- | --- |
| default | 默认液态玻璃 |
| clear | 纯透明液态玻璃 |
| sketch | 简笔画 |
| anime | 纯二次元 |
| seasons | 四季动态山湖，每季 1 分钟 |

「山水四季」已移除，`alpine` 不能用于新主题。旧站点及旧备份中的该选项会迁移到 `seasons`。

`seasons` 使用内置动态风景，忽略自定义背景图片；背景遮罩、图标、配色和局部样式仍然有效。它使用浏览器合成动画与逐帧调度，目标为 60 FPS，保留季节颜色变化、暂停按钮和减少动态效果偏好。切到后台时暂停动画，返回后继续。

## 浅色与深色配色

`palette.light` 与 `palette.dark` 各为颜色映射。未填写的项继承基础主题。JSON 键名使用下表中的名字，不带 `--` 前缀。

| 键名 | 影响的部分 |
| --- | --- |
| bg | 页面底色 |
| text | 主要文字 |
| muted | 次要文字 |
| quiet | 提示文字 |
| glass | 普通卡片表面 |
| glassStrong | 较实的卡片和工具栏表面 |
| inset | 卡片内部区域 |
| inputBg | 输入框背景 |
| edge | 卡片外边框 |
| line | 分隔线 |
| accent | 强调色 |
| accentWash | 浅强调底色 |
| track | 指标槽背景 |
| dialog | 普通弹窗背景 |
| chart | 图表和指标默认颜色 |
| focus | 键盘焦点颜色 |
| terminalSurface | SSH 工作区背景 |
| buttonColor | 主按钮背景 |
| buttonText | 主按钮文字 |

- 颜色使用 `#RGB`、`#RRGGBB`；表面色还可使用带透明度的 `#RRGGBBAA` 或 `transparent`。
- `text`、`muted`、`quiet`、`accent`、`chart`、`focus`、`buttonColor`、`buttonText` 必须是不透明的十六进制颜色。
- `theme.accent` 可为空，表示使用基础主题／配色表的强调色。若填写六位颜色，将优先作为全站主题色，并由面板调整文字对比度；希望浅深色使用不同强调色时，将其留空。
- 四季主题的普通指标继续按季节变化，CPU/内存等告警颜色不会被季节色覆盖。

## 背景图片与站点图标

`background` 和 `favicon` 填 `null` 使用默认资源；自定义图片使用以下格式：

```json
{ "mime": "image/png", "data": "图片原始字节的标准 Base64，不包含 data: 前缀" }
```

支持 PNG、JPG、WebP。MIME 分别为 `image/png`、`image/jpeg`、`image/webp`。服务端识别真实图片类型、验证尺寸并重新编码，不会原样保存图片尾部的附加内容。

| 项目 | 背景 | 图标 |
| --- | --- | --- |
| 原始图片文件大小 | 最大 1536 KiB | 最大 192 KiB |
| 输入宽高 | 各不超过 4096 像素 | 各不超过 1024 像素 |
| 保存最大边长 | 1920 像素 | 192 像素 |
| 保存格式 | JPEG | PNG |

总 JSON 文件仍需小于 3 MiB；Base64 会增加约三分之一体积。推荐预先压缩背景图片。`backgroundDim` 为 0–85 的整数（遮罩百分比），`backgroundBlur` 为 0–24 的整数（像素），`brandIcon` 控制是否同时把站点图标用于左上角标识。

使用 Python 为主题加入图片：

```python
import base64
import json
from pathlib import Path

file = Path("my-theme.yuji-theme.json")
theme = json.loads(file.read_text(encoding="utf-8"))
theme["theme"]["background"] = {
    "mime": "image/jpeg",
    "data": base64.b64encode(Path("background.jpg").read_bytes()).decode("ascii")
}
file.write_text(json.dumps(theme, ensure_ascii=False, indent=2), encoding="utf-8")
```

请使用有权分享的图片。主题包中的图标在启用后会公开展示。

## 局部样式

`customCss` 最大 4096 字节，最多 16 组规则，每组最多 12 条声明。每组只能使用一个指定选择器；样式由服务器解析后再作用到界面。

选择器：`.server-card`、`.node-title`、`.card-footer`、`.toolbar`、`.brand-mark`、`.brand-name`、`.footer`、`.admin-box`、`.node-dialog`。其中 `.node-dialog` 只作用于普通弹窗，SSH 使用主题配色中的 `terminalSurface`。

| 属性 | 可用值 |
| --- | --- |
| color / background-color / border-color | 十六进制颜色或 transparent |
| border-radius | 0–40px，可写 1–4 个值 |
| border-width | 0–3px |
| font-size | 11–32px |
| font-weight | 400、500、600、700 |
| letter-spacing | 0–4px |

```css
.server-card { border-radius: 18px; border-width: 1px; }
.node-title { font-size: 16px; font-weight: 600; }
.admin-box { border-radius: 20px; }
```

不支持外部 `url()`、`@import`、脚本、HTML、SVG、任意选择器、隐藏元素、定位覆盖层、`!important` 或 CSS 转义。主题是外观数据包，不提供 JavaScript／PHP 插件入口，也不能改变后台接口、权限和认证。

## 分享前检查

- 同时检查浅色与深色模式，主要文字保持清晰可读。
- 检查手机宽度、服务器卡片、管理表单、登录窗口和 SSH 终端。
- 保留明显的离线、告警、错误和键盘焦点状态。
- 修改一个节点指标时，卡片大小和操作按钮不应跳动。
- 从后台重新导出并再次导入，确认图片、配色、作者和版本一致。
- 使用带 UTF-8 中文名称的主题包检查文字显示。

主题文件导入后，只有点击保存才替换当前站点主题。误操作可重新选择内置主题并保存，或使用「恢复默认设置」后保存。当前启用的主题随面板加密备份一起保存和恢复。

配色表中的 `bg` 会替换所选基础主题的环境底色；上传背景图时优先显示背景图，四季主题仍显示动态风景。四季的模糊与暗色效果预先绘入缓存，透明卡片不对运动背景逐帧执行模糊。
