<?php
declare(strict_types=1);
// SERVER_NAME and SERVER_PORT are supplied from trusted Nginx configuration.
// Never use the first visitor's Host header in the private ownership link.
function probe_setup_origin():string {
 $name=strtolower((string)($_SERVER['SERVER_NAME']??''));$port=(string)($_SERVER['SERVER_PORT']??'');
 if(strlen($name)>253||!preg_match('/^[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?$/D',$name)||!preg_match('/^[0-9]{1,5}$/D',$port)||(int)$port<1||(int)$port>65535)throw new ProbeError('请在 Nginx server_name 配置本站的具体域名',503);
 return 'https://'.$name.((int)$port===443?'':':'.(int)$port);
}
function probe_requirements():array {
 return ['PHP 8.0 或更新版本'=>PHP_VERSION_ID>=80000,'cURL / OpenSSL / Session'=>extension_loaded('curl')&&extension_loaded('openssl')&&extension_loaded('session'),'后台进程支持'=>function_exists('proc_open')&&function_exists('proc_close'),'配置目录可写'=>is_writable(PROBE_STORAGE),'预编译通信程序'=>is_file(probe_binary()),'HTTPS'=>probe_secure()];
}
function probe_setup():array {
 if(!is_dir(PROBE_STORAGE))throw new ProbeError('请将 storage 目录权限设置为 PHP 运行用户可写',503);
 probe_session();$guard=fopen(PROBE_STORAGE.'/install.lock','c');
 if(!$guard||!flock($guard,LOCK_EX))throw new ProbeError('安装锁不可用',503);
 try{
  if(probe_installed())throw new ProbeError('站点已安装',409);
  $file=PROBE_STORAGE.'/setup.json';
  if(!is_file($file)){
   $origin=probe_setup_origin();$key=bin2hex(random_bytes(32));
   probe_atomic($file,['hash'=>hash('sha256',$key)]);
   file_put_contents(PROBE_STORAGE.'/setup-link.txt',$origin.'/?setup_key='.$key."\n",LOCK_EX);
   chmod(PROBE_STORAGE.'/setup-link.txt',0600);
  }
  $setup=probe_read($file);$key=$_GET['setup_key']??null;
  if(is_string($key)&&strlen($key)===64&&hash_equals($setup['hash'],hash('sha256',$key))){
   session_regenerate_id(true);$_SESSION['setup_owner']=$setup['hash'];
   header('Location: /',true,303);exit;
  }
  $owner=isset($_SESSION['setup_owner'])&&hash_equals($setup['hash'],$_SESSION['setup_owner']);
  return ['owner'=>$owner,'csrf'=>$_SESSION['csrf'],'checks'=>probe_requirements()];
 }finally{flock($guard,LOCK_UN);fclose($guard);}
}
function probe_install(array $setup):void {
 if(!$setup['owner'])throw new ProbeError('请先使用服务器内的一次性安装链接',403);
 if(($_SERVER['HTTP_ORIGIN']??'')!==probe_origin()||!hash_equals($setup['csrf'],$_SERVER['HTTP_X_CSRF_TOKEN']??''))throw new ProbeError('安装会话已失效，请刷新页面',403);
 if(in_array(false,$setup['checks'],true))throw new ProbeError('请先完成运行环境检查',503);
 require_once PROBE_ROOT.'/app/install-state.php';
 probe_initialize(probe_body(),probe_origin());
 $_SESSION=[];session_destroy();setcookie('__Host-probe_setup','',['expires'=>1,'path'=>'/','secure'=>true,'httponly'=>true,'samesite'=>'Strict']);
 probe_json(['ok'=>true,'redirect'=>'/?login=1']);
}
