<?php
declare(strict_types=1);
function probe_forward(string $path,array $config):void {
 if(!preg_match('#^/api/(?:session|login|logout|public/nodes|migrate|admin/(?:node-order|nodes(?:/[a-zA-Z0-9_-]+)?|renewals/[a-zA-Z0-9_-]{1,64}|site|telegram(?:/(?:test|preview))?|password|reauth|ops/[a-zA-Z0-9_/-]+|audit|commands(?:/[a-zA-Z0-9_-]+)?|terminal-ticket|inspect-ssh|trust-ssh|deploy(?:/[a-zA-Z0-9_-]+)?|mfa/(?:setup|enable|disable)))$#D',$path))throw new ProbeError('接口不存在',404);
 $method=$_SERVER['REQUEST_METHOD']??'GET';if(!in_array($method,['GET','POST','PUT','PATCH','DELETE'],true))throw new ProbeError('请求方法不正确',405);
 probe_start($config);$headers=['Host: '.parse_url($config['origin'],PHP_URL_HOST),'X-Probe-Gateway: '.probe_gateway_key()];
 $ip=$_SERVER['REMOTE_ADDR']??'';if(filter_var($ip,FILTER_VALIDATE_IP))$headers[]='X-Real-IP: '.$ip;
 foreach(['CONTENT_TYPE'=>'Content-Type','HTTP_COOKIE'=>'Cookie','HTTP_USER_AGENT'=>'User-Agent','HTTP_ORIGIN'=>'Origin','HTTP_X_CSRF_TOKEN'=>'X-CSRF-Token','HTTP_SEC_FETCH_SITE'=>'Sec-Fetch-Site'] as $key=>$name){$value=$_SERVER[$key]??'';if(is_string($value)&&$value!==''&&!str_contains($value,"\r")&&!str_contains($value,"\n")&&strlen($value)<=8192)$headers[]=$name.': '.$value;}
 $limit=$path==='/api/admin/ops/restore'?24*1024*1024:16384;$body=file_get_contents('php://input',false,null,0,$limit+1);if($body===false||strlen($body)>$limit)throw new ProbeError('请求内容过大',413);
 $responseHeaders=[];$buffer='';$tooLarge=false;$ch=curl_init('http://'.$config['listen'].$path);
 curl_setopt_array($ch,[CURLOPT_CUSTOMREQUEST=>$method,CURLOPT_HTTPHEADER=>$headers,CURLOPT_POSTFIELDS=>$method==='GET'?null:$body,CURLOPT_CONNECTTIMEOUT=>1,CURLOPT_TIMEOUT=>str_starts_with($path,'/api/admin/ops/offsite/')?50:20,CURLOPT_PROXY=>'',CURLOPT_FOLLOWLOCATION=>false,
  CURLOPT_HEADERFUNCTION=>static function($ch,string $line)use(&$responseHeaders):int{if(str_contains($line,':')){[$key,$value]=explode(':',$line,2);if(in_array(strtolower($key),['content-type','set-cookie','retry-after'],true))$responseHeaders[]=trim($key).': '.trim($value);}return strlen($line);},
  CURLOPT_WRITEFUNCTION=>static function($ch,string $data)use(&$buffer,&$tooLarge):int{if(strlen($buffer)+strlen($data)>32*1024*1024){$tooLarge=true;return 0;}$buffer.=$data;return strlen($data);}]);
 $ok=curl_exec($ch);$status=curl_getinfo($ch,CURLINFO_RESPONSE_CODE);curl_close($ch);
 if($ok===false||$tooLarge||$status<200||$status>599)throw new ProbeError('通信服务响应异常，请稍后重试',502);
 http_response_code($status);foreach($responseHeaders as $line)header($line,false);echo $buffer;exit;
}
