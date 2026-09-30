<?php
declare(strict_types=1);
require __DIR__.'/../app/bootstrap.php';
try{
 $path=parse_url($_SERVER['REQUEST_URI']??'/',PHP_URL_PATH);if(!is_string($path))throw new ProbeError('请求地址不正确');
 if(!probe_secure())throw new ProbeError('请先为站点启用 HTTPS',400);
 if(!probe_installed()){
  require PROBE_ROOT.'/app/install.php';$setup=probe_setup();
  if($path==='/install'&&($_SERVER['REQUEST_METHOD']??'GET')==='POST')probe_install($setup);
  if($path!=='/'||($_SERVER['REQUEST_METHOD']??'GET')!=='GET')throw new ProbeError('请先完成安装',404);
  header('Content-Type: text/html; charset=utf-8');require PROBE_ROOT.'/app/setup-view.php';exit;
 }
 $config=probe_settings();if(probe_origin()!==$config['origin'])throw new ProbeError('请使用安装时设置的站点域名访问',421);
 if($path==='/install')throw new ProbeError('站点已安装，安装入口已锁定',409);
 if(isset($_GET['probe_wake'])){probe_start($config);header('Retry-After: 2');probe_json(['error'=>'通信服务正在恢复，请重新连接'],503);}
 if(str_starts_with($path,'/api/')){require PROBE_ROOT.'/app/gateway.php';probe_forward($path,$config);}
 if($path!=='/'||!in_array($_SERVER['REQUEST_METHOD']??'GET',['GET','HEAD'],true))throw new ProbeError('页面不存在',404);
 probe_start($config);header('Content-Type: text/html; charset=utf-8');if(($_SERVER['REQUEST_METHOD']??'GET')!=='HEAD'){
  $data=probe_read(PROBE_DATA.'/nodes.json');
  $name=$data['site']['name']??'服务器监控';
  if(!is_string($name)||trim($name)==='')$name='服务器监控';
  $view=@file_get_contents(PROBE_ROOT.'/app/view.html');
  if($view===false)throw new ProbeError('页面模板不可用',503);
  // Site names are text in both element content and quoted attributes.
  echo str_replace('__SITE_NAME__',htmlspecialchars($name,ENT_QUOTES|ENT_SUBSTITUTE,'UTF-8'),$view);
 }
}catch(Throwable $e){
 $status=$e instanceof ProbeError?$e->status:500;$message=$e instanceof ProbeError?$e->getMessage():'操作未完成，请查看服务器错误日志';
 if(!($e instanceof ProbeError))error_log('Probe: '.get_class($e).': '.$e->getMessage());
 probe_json(['error'=>$message],$status);
}
