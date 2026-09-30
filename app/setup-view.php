<?php declare(strict_types=1);function h(string $s):string{return htmlspecialchars($s,ENT_QUOTES|ENT_SUBSTITUTE,'UTF-8');} ?>
<!doctype html>
<html lang="zh-CN"><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>安装 · Vistart Probe</title><link rel="stylesheet" href="/setup.css"><script type="module" src="/setup.js"></script></head>
<body><main class="setup-card"><span class="brand">VISTART <span>PROBE</span></span><div class="mark" aria-hidden="true">◉</div><h1><?= $setup['owner']?'设置你的探针看板':'准备安装' ?></h1><p class="intro"><?= $setup['owner']?'填写站点和管理员信息，即可开始使用。':'首次安装需要确认你拥有这台服务器。' ?></p>
<?php if(!$setup['owner']): ?>
<div class="notice">请在宝塔文件管理器中打开站点目录下的 <code>storage/setup-link.txt</code>，复制其中的链接到浏览器继续安装。</div>
<p class="muted">安装链接保存在服务器本地，完成后自动删除。</p>
<?php else: ?>
<form id="setup-form" data-csrf="<?= h($setup['csrf']) ?>">
<label>站点名称<input name="name" maxlength="60" value="服务器监测" autocomplete="organization" required></label>
<label>管理员用户名<input name="username" minlength="1" maxlength="32" value="admin" pattern="[a-zA-Z_][a-zA-Z0-9_.-]{0,31}" autocomplete="username" required></label>
<label>管理员密码<input name="password" type="password" minlength="12" maxlength="72" autocomplete="new-password" required placeholder="至少 12 位"></label>
<p id="setup-error" role="alert" hidden></p><button type="submit" <?= in_array(false,$setup['checks'],true)?'disabled':'' ?>>完成安装</button>
</form>
<?php endif; ?>
<details <?= in_array(false,$setup['checks'],true)?'open':'' ?>><summary>运行环境检查</summary><ul><?php foreach($setup['checks'] as $label=>$ok): ?><li><span><?= h($label) ?></span><b class="<?= $ok?'ok':'bad' ?>"><?= $ok?'就绪':'待配置' ?></b></li><?php endforeach; ?></ul></details><footer>无需数据库服务 · 配置保存在本机</footer></main></body></html>
