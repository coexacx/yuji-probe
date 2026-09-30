<?php
declare(strict_types=1);
// Passwords arrive on stdin, never in argv, environment variables or a file.
if(PHP_SAPI!=='cli'){http_response_code(404);exit(1);}
require __DIR__.'/../app/bootstrap.php';
require PROBE_ROOT.'/app/install-state.php';
try{
 if($argc!==1)throw new RuntimeException('Usage: credentials must be supplied on stdin');
 if(!extension_loaded('curl')||!extension_loaded('openssl')||!function_exists('proc_open'))throw new RuntimeException('PHP runtime is incomplete');
 $raw=stream_get_contents(STDIN,1025);
 if(!is_string($raw)||strlen($raw)>1024)throw new RuntimeException('Invalid input');
 $fields=explode("\0",$raw);
 if(count($fields)!==5||$fields[4]!=='')throw new RuntimeException('Invalid input');
 [$name,$user,$password,$domain]=$fields;
 if(!preg_match('//u',$name))throw new RuntimeException('Site name must be UTF-8');
 if(strlen($domain)>253||!preg_match('/^(?=.{1,253}$)(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z](?:[a-z0-9-]{0,61}[a-z0-9])?$/D',$domain))throw new RuntimeException('Invalid domain');
 if(!is_dir(PROBE_STORAGE)||!is_writable(PROBE_STORAGE))throw new RuntimeException('Private storage unavailable');
 probe_initialize(['name'=>$name,'username'=>$user,'password'=>$password],'https://'.$domain,false);
 if(function_exists('sodium_memzero'))sodium_memzero($password);
 unset($raw,$fields);
 fwrite(STDOUT,"Administrator and site initialized.\n");
}catch(Throwable $e){fwrite(STDERR,($e instanceof ProbeError?$e->getMessage():'Initialization failed; check input and private storage permissions')."\n");exit(1);}
