<?php
declare(strict_types=1);
const PROBE_ROOT = __DIR__.'/..';
const PROBE_STORAGE = PROBE_ROOT.'/storage';
const PROBE_DATA = PROBE_STORAGE.'/control';
ini_set('display_errors','0');
ini_set('log_errors','1');
umask(0077);
header('Cache-Control: no-store');
header('X-Content-Type-Options: nosniff');
header('X-Frame-Options: DENY');
header('Referrer-Policy: no-referrer');
header("Content-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self'; font-src 'self'; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'");
header('Permissions-Policy: camera=(), microphone=(), geolocation=()');
final class ProbeError extends RuntimeException {public int $status; public function __construct(string $message,int $status=400){parent::__construct($message);$this->status=$status;}}
function probe_json(array $value,int $status=200):void {http_response_code($status);header('Content-Type: application/json; charset=utf-8');echo json_encode($value,JSON_UNESCAPED_UNICODE|JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR);exit;}
function probe_read(string $file):array { $raw=@file_get_contents($file);if($raw===false)throw new ProbeError('配置文件不可用',503);$v=json_decode($raw,true,64,JSON_THROW_ON_ERROR);if(!is_array($v))throw new ProbeError('配置文件不正确',503);return $v; }
function probe_atomic(string $file,array $value):void {
 $tmp=$file.'.'.bin2hex(random_bytes(8));$data=json_encode($value,JSON_UNESCAPED_UNICODE|JSON_UNESCAPED_SLASHES|JSON_THROW_ON_ERROR);
 if(file_put_contents($tmp,$data,LOCK_EX)===false)throw new ProbeError('无法写入配置目录',503);
 chmod($tmp,0600);if(!rename($tmp,$file)){@unlink($tmp);throw new ProbeError('配置保存失败',503);}
}
function probe_host():string { $host=$_SERVER['HTTP_HOST']??'';if(!preg_match('/^[a-zA-Z0-9.-]+(?::[0-9]{1,5})?$/D',$host))throw new ProbeError('站点域名不正确');return strtolower($host); }
function probe_secure():bool{return isset($_SERVER['HTTPS'])&&strtolower((string)$_SERVER['HTTPS'])!=='off'&&$_SERVER['HTTPS']!=='';}
function probe_origin():string {if(!probe_secure())throw new ProbeError('请先在宝塔为站点启用 HTTPS',400);return 'https://'.probe_host();}
function probe_installed():bool {return is_file(PROBE_STORAGE.'/installed.json');}
function probe_settings():array{return probe_read(PROBE_STORAGE.'/installed.json');}
function probe_listen():string {
 $value=getenv('PROBE_LISTEN')?:'127.0.0.1:19281';
 if(!preg_match('/^127\.0\.0\.1:([1-9][0-9]{3,4})$/D',$value,$m)||(int)$m[1]>65535)throw new ProbeError('通信服务监听配置不正确',503);
 return $value;
}
function probe_binary():string {
 $arch=php_uname('m');if(!in_array($arch,['x86_64','amd64','aarch64','arm64'],true))throw new ProbeError('暂不支持此服务器架构');
 return PROBE_ROOT.'/bin/probe-linux-'.(in_array($arch,['aarch64','arm64'],true)?'arm64':'amd64');
}
function probe_gateway_key():string {$key=@file_get_contents(PROBE_DATA.'/app.key');if($key===false||strlen($key)!==32)throw new ProbeError('通信密钥不可用',503);return hash('sha256','vistart-probe-php-gateway-v1:'.$key);}
function probe_health(array $config):bool {
 try{$key=probe_gateway_key();}catch(Throwable $e){return false;}
 $ch=curl_init('http://'.$config['listen'].'/_internal/health');curl_setopt_array($ch,[CURLOPT_RETURNTRANSFER=>true,CURLOPT_CONNECTTIMEOUT_MS=>300,CURLOPT_TIMEOUT_MS=>700,CURLOPT_PROXY=>'',CURLOPT_HTTPHEADER=>['X-Probe-Gateway: '.$key],CURLOPT_FOLLOWLOCATION=>false]);
 $raw=curl_exec($ch);$code=curl_getinfo($ch,CURLINFO_RESPONSE_CODE);curl_close($ch);if($code!==200||!is_string($raw)||strlen($raw)>2048)return false;
 $data=json_decode($raw,true);return is_array($data)&&($data['ok']??false)&&($data['service']??'')==='vistart-probe'&&($data['origin']??'')===$config['origin'];
}
function probe_start(array $config):void {
 if(probe_health($config))return;
 if(getenv('PROBE_SERVICE_MANAGED')==='1'){header('Retry-After: 2');throw new ProbeError('通信服务正在恢复，请稍后重试',503);}
 if(!function_exists('proc_open'))throw new ProbeError('请在宝塔 PHP 设置中允许 proc_open 启动随包通信服务',503);
 $lock=fopen(PROBE_STORAGE.'/startup.lock','c');if(!$lock||!flock($lock,LOCK_EX|LOCK_NB))throw new ProbeError('通信服务正在启动，请稍后重试',503);
 try{
  if(probe_health($config))return;
  $last=PROBE_STORAGE.'/last-start';if(is_file($last)&&time()-(int)file_get_contents($last)<5)throw new ProbeError('通信服务正在启动，请稍后重试',503);
  file_put_contents($last,(string)time(),LOCK_EX);chmod($last,0600);
  $binary=probe_binary();if(!is_file($binary))throw new ProbeError('缺少预编译通信程序',503);if(!is_executable($binary)&&!chmod($binary,0700))throw new ProbeError('无法设置通信程序执行权限',503);
  $command=[$binary,'-state',PROBE_DATA,'-listen',$config['listen'],'-origin',$config['origin'],'-php-gateway','-daemon'];
  $proc=proc_open($command,[0=>['pipe','r'],1=>['file',PROBE_STORAGE.'/startup.log','a'],2=>['file',PROBE_STORAGE.'/startup.log','a']],$pipes,PROBE_ROOT,['PATH'=>'/usr/bin:/bin','GOMAXPROCS'=>'2','GOMEMLIMIT'=>'192MiB']);
  if(!is_resource($proc))throw new ProbeError('无法启动通信服务',503);fclose($pipes[0]);$code=proc_close($proc);if($code!==0)throw new ProbeError('通信服务启动失败，请查看本地启动日志',503);
  for($i=0;$i<25;$i++){usleep(100000);if(probe_health($config))return;}
  throw new ProbeError('通信服务尚未就绪，请检查执行权限和端口占用',503);
 }finally{flock($lock,LOCK_UN);fclose($lock);}
}
function probe_session():void {
 if(session_status()===PHP_SESSION_ACTIVE)return;
 if(!is_dir(PROBE_STORAGE.'/sessions')&&!mkdir(PROBE_STORAGE.'/sessions',0700,true))throw new ProbeError('安装目录不可写',503);
 session_save_path(PROBE_STORAGE.'/sessions');session_name('__Host-probe_setup');
 session_set_cookie_params(['lifetime'=>0,'path'=>'/','secure'=>true,'httponly'=>true,'samesite'=>'Strict']);
 ini_set('session.use_strict_mode','1');ini_set('session.use_only_cookies','1');session_start();
 if(!isset($_SESSION['csrf']))$_SESSION['csrf']=bin2hex(random_bytes(32));
}
function probe_body():array {if(!str_starts_with($_SERVER['CONTENT_TYPE']??'','application/json'))throw new ProbeError('请使用 JSON 请求',415);$body=file_get_contents('php://input',false,null,0,16385);if($body===false||strlen($body)>16384)throw new ProbeError('请求内容过大',413);$v=json_decode($body,true,32,JSON_THROW_ON_ERROR);if(!is_array($v))throw new ProbeError('请求格式不正确');return $v;}
function probe_text(array $v,string $key,int $max):string {$s=$v[$key]??null;if(!is_string($s)||trim($s)===''||strlen($s)>$max||preg_match('/[\x00-\x1f\x7f]/',$s))throw new ProbeError('请检查填写内容');return trim($s);}
