<?php
declare(strict_types=1);
// Used by the authenticated web wizard and the server-local CLI only.
function probe_initialize(array $body,string $origin,bool $start=true):array {
 $name=probe_text($body,'name',180);$user=probe_text($body,'username',32);$password=$body['password']??null;
 if(!preg_match('/^[a-zA-Z_][a-zA-Z0-9_.-]{0,31}$/D',$user))throw new ProbeError('管理员用户名格式不正确');
 if(!is_string($password)||strlen($password)<12||strlen($password)>72||str_contains($password,"\0"))throw new ProbeError('密码长度需为 12 至 72 字节');
 $lock=fopen(PROBE_STORAGE.'/install.lock','c');
 if(!$lock||!flock($lock,LOCK_EX|LOCK_NB))throw new ProbeError('安装正在进行，请稍后重试',409);
 try{
  if(probe_installed())throw new ProbeError('站点已安装',409);
  $config=['schema'=>1,'origin'=>$origin,'listen'=>probe_listen()];
  if(is_dir(PROBE_DATA)){
   $pending=probe_read(PROBE_STORAGE.'/pending.json');$auth=probe_read(PROBE_DATA.'/auth.json');
   if($pending!==$config||$auth['username']!==$user||!password_verify($password,$auth['hash']))throw new ProbeError('已有安装配置，请使用上次填写的管理员信息重试',409);
  }else{
   $stage=PROBE_STORAGE.'/.install-'.bin2hex(random_bytes(8));
   if(!mkdir($stage,0700))throw new ProbeError('无法创建配置目录',503);
   probe_atomic($stage.'/auth.json',['username'=>$user,'hash'=>password_hash($password,PASSWORD_BCRYPT,['cost'=>12]),'version'=>bin2hex(random_bytes(32))]);
   probe_atomic($stage.'/nodes.json',['schema'=>2,'preview'=>false,'site'=>['name'=>$name,'public'=>true,'refreshSeconds'=>5],'nodes'=>[],'secrets'=>(object)[],'commands'=>[]]);
   file_put_contents($stage.'/app.key',random_bytes(32),LOCK_EX);chmod($stage.'/app.key',0600);
   probe_atomic(PROBE_STORAGE.'/pending.json',$config);
   if(!rename($stage,PROBE_DATA))throw new ProbeError('配置目录初始化失败',503);
  }
  if($start)probe_start($config);probe_atomic(PROBE_STORAGE.'/installed.json',$config);
  @unlink(PROBE_STORAGE.'/pending.json');@unlink(PROBE_STORAGE.'/setup-link.txt');@unlink(PROBE_STORAGE.'/setup.json');
  return $config;
 }finally{flock($lock,LOCK_UN);fclose($lock);}
}
