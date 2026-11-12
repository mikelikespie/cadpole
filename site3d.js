var th=0,Wl=1,nh=2;var Ar=1,ih=2,bs=3,Jn=0,Dt=1,sn=2,qn=0,Ss=1,Hl=2,Yl=3,Zl=4,sh=5;var Xi=100,rh=101,oh=102,ah=103,lh=104,ch=200,uh=201,hh=202,dh=203,Xl=204,Kl=205,fh=206,ph=207,mh=208,gh=209,xh=210,yh=211,_h=212,bh=213,Sh=214,ro=0,oo=1,ao=2,us=3,lo=4,co=5,uo=6,ho=7,$l=0,vh=1,Ah=2,pn=0,Jl=1,ql=2,Ql=3,jl=4,Th=5,ec=6,Tr=7;var tc=300,hi=301,Bi=302,vs=303,No=304,Er=306,ar=1e3,Ln=1001,lr=1002,Rt=1003,Eh=1004;var Mr=1005;var Bt=1006,Io=1007;var di=1008;var mn=1009,nc=1010,ic=1011,As=1012,Ts=1013,bn=1014,Dn=1015,Sn=1016,Bo=1017,Fo=1018,Es=1020,sc=35902,rc=35899,Lo=1021,Uo=1022,gn=1023,Un=1026,fi=1027,Do=1028,Rr=1029,Qn=1030,wr=1031,Mh=1032,Po=1033,Cr=33776,Nr=33777,Ir=33778,Br=33779,Go=35840,Vo=35841,Oo=35842,ko=35843,zo=36196,Wo=37492,Ho=37496,Yo=37488,Zo=37489,Ms=37490,Xo=37491,Ko=37808,$o=37809,Jo=37810,qo=37811,Qo=37812,jo=37813,ea=37814,ta=37815,na=37816,ia=37817,sa=37818,ra=37819,oa=37820,aa=37821,la=36492,ca=36494,ua=36495,ha=36283,da=36284,Rs=36285,fa=36286;var fo=2300,Al=2301,vl=2302,Ku=2303,$u=2400,Ju=2401,qu=2402;var qp=3200;var Ki=0,oc=1,Pn="",yn="srgb",Wi="srgb-linear",cr="linear",nt="srgb",pa="",ac="rg",Rh="ga";var io=7680;var wh=519,lc=512,ma=513,cc=514,ws=515,Fr=516,uc=517,$i=518,hc=519,Lr=35044;var Ch="300 es",hn=2e3,li=2001;var Nh={TEXTURE_COMPARE:"depthTextureCompare"};function R0(n){for(let e=n.length-1;e>=0;--e)if(n[e]>=65535)return!0;return!1}function Qp(n){return ArrayBuffer.isView(n)&&!(n instanceof DataView)}function po(n){return document.createElementNS("http://www.w3.org/1999/xhtml",n)}function Ih(){let n=po("canvas");return n.style.display="block",n}var Ap={},ur=null;function hs(...n){let e="THREE."+n.shift();ur?ur("log",e,...n):console.log(e,...n)}function jp(n){let e=n[0];if(typeof e=="string"&&e.startsWith("TSL:")){let t=n[1];t&&t.isStackTrace?n[0]+=" "+t.getLocation():n[1]='Stack trace not available. Enable "THREE.Node.captureStackTrace" to capture stack traces.'}return n}function pe(...n){n=jp(n);let e="THREE."+n.shift();if(ur)ur("warn",e,...n);else{let t=n[0];t&&t.isStackTrace?console.warn(t.getError(e)):console.warn(e,...n)}}function _e(...n){n=jp(n);let e="THREE."+n.shift();if(ur)ur("error",e,...n);else{let t=n[0];t&&t.isStackTrace?console.error(t.getError(e)):console.error(e,...n)}}function Zn(...n){let e=n.join(" ");e in Ap||(Ap[e]=!0,pe(...n))}function em(n,e,t){return new Promise(function(i,s){function r(){switch(n.clientWaitSync(e,n.SYNC_FLUSH_COMMANDS_BIT,0)){case n.WAIT_FAILED:s();break;case n.TIMEOUT_EXPIRED:setTimeout(r,t);break;default:i()}}setTimeout(r,t)})}var Bh={[ro]:oo,[ao]:uo,[lo]:ho,[us]:co,[oo]:ro,[uo]:ao,[ho]:lo,[co]:us},wn=class{addEventListener(e,t){this._listeners===void 0&&(this._listeners={});let i=this._listeners;i[e]===void 0&&(i[e]=[]),i[e].indexOf(t)===-1&&i[e].push(t)}hasEventListener(e,t){let i=this._listeners;return i===void 0?!1:i[e]!==void 0&&i[e].indexOf(t)!==-1}removeEventListener(e,t){let i=this._listeners;if(i===void 0)return;let s=i[e];if(s!==void 0){let r=s.indexOf(t);r!==-1&&s.splice(r,1)}}dispatchEvent(e){let t=this._listeners;if(t===void 0)return;let i=t[e.type];if(i!==void 0){e.target=this;let s=i.slice(0);for(let r=0,o=s.length;r<o;r++)s[r].call(this,e);e.target=null}}},en=["00","01","02","03","04","05","06","07","08","09","0a","0b","0c","0d","0e","0f","10","11","12","13","14","15","16","17","18","19","1a","1b","1c","1d","1e","1f","20","21","22","23","24","25","26","27","28","29","2a","2b","2c","2d","2e","2f","30","31","32","33","34","35","36","37","38","39","3a","3b","3c","3d","3e","3f","40","41","42","43","44","45","46","47","48","49","4a","4b","4c","4d","4e","4f","50","51","52","53","54","55","56","57","58","59","5a","5b","5c","5d","5e","5f","60","61","62","63","64","65","66","67","68","69","6a","6b","6c","6d","6e","6f","70","71","72","73","74","75","76","77","78","79","7a","7b","7c","7d","7e","7f","80","81","82","83","84","85","86","87","88","89","8a","8b","8c","8d","8e","8f","90","91","92","93","94","95","96","97","98","99","9a","9b","9c","9d","9e","9f","a0","a1","a2","a3","a4","a5","a6","a7","a8","a9","aa","ab","ac","ad","ae","af","b0","b1","b2","b3","b4","b5","b6","b7","b8","b9","ba","bb","bc","bd","be","bf","c0","c1","c2","c3","c4","c5","c6","c7","c8","c9","ca","cb","cc","cd","ce","cf","d0","d1","d2","d3","d4","d5","d6","d7","d8","d9","da","db","dc","dd","de","df","e0","e1","e2","e3","e4","e5","e6","e7","e8","e9","ea","eb","ec","ed","ee","ef","f0","f1","f2","f3","f4","f5","f6","f7","f8","f9","fa","fb","fc","fd","fe","ff"],Tp=1234567,rr=Math.PI/180,ds=180/Math.PI;function Ci(){let n=Math.random()*4294967295|0,e=Math.random()*4294967295|0,t=Math.random()*4294967295|0,i=Math.random()*4294967295|0;return(en[n&255]+en[n>>8&255]+en[n>>16&255]+en[n>>24&255]+"-"+en[e&255]+en[e>>8&255]+"-"+en[e>>16&15|64]+en[e>>24&255]+"-"+en[t&63|128]+en[t>>8&255]+"-"+en[t>>16&255]+en[t>>24&255]+en[i&255]+en[i>>8&255]+en[i>>16&255]+en[i>>24&255]).toLowerCase()}function qe(n,e,t){return Math.max(e,Math.min(t,n))}function Fh(n,e){return(n%e+e)%e}function w0(n,e,t,i,s){return i+(n-e)*(s-i)/(t-e)}function C0(n,e,t){return n!==e?(t-n)/(e-n):0}function so(n,e,t){return(1-t)*n+t*e}function N0(n,e,t,i){return so(n,e,1-Math.exp(-t*i))}function I0(n,e=1){return e-Math.abs(Fh(n,e*2)-e)}function B0(n,e,t){return n<=e?0:n>=t?1:(n=(n-e)/(t-e),n*n*(3-2*n))}function F0(n,e,t){return n<=e?0:n>=t?1:(n=(n-e)/(t-e),n*n*n*(n*(n*6-15)+10))}function L0(n,e){return n+Math.floor(Math.random()*(e-n+1))}function U0(n,e){return n+Math.random()*(e-n)}function D0(n){return n*(.5-Math.random())}function P0(n){n!==void 0&&(Tp=n);let e=Tp+=1831565813;return e=Math.imul(e^e>>>15,e|1),e^=e+Math.imul(e^e>>>7,e|61),((e^e>>>14)>>>0)/4294967296}function G0(n){return n*rr}function V0(n){return n*ds}function O0(n){return n>0&&Number.isInteger(n)&&2**Math.round(Math.log2(n))===n}function k0(n){return Math.pow(2,Math.ceil(Math.log(n)/Math.LN2))}function z0(n){return Math.pow(2,Math.floor(Math.log(n)/Math.LN2))}function W0(n,e,t,i,s){let r=Math.cos,o=Math.sin,a=r(t/2),l=o(t/2),c=r((e+i)/2),u=o((e+i)/2),d=r((e-i)/2),h=o((e-i)/2),f=r((i-e)/2),g=o((i-e)/2);switch(s){case"XYX":n.set(a*u,l*d,l*h,a*c);break;case"YZY":n.set(l*h,a*u,l*d,a*c);break;case"ZXZ":n.set(l*d,l*h,a*u,a*c);break;case"XZX":n.set(a*u,l*g,l*f,a*c);break;case"YXY":n.set(l*f,a*u,l*g,a*c);break;case"ZYZ":n.set(l*g,l*f,a*u,a*c);break;default:pe("MathUtils: .setQuaternionFromProperEuler() encountered an unknown order: "+s)}}function Hn(n,e){switch(e.constructor){case Float32Array:return n;case Uint32Array:return n/4294967295;case Uint16Array:return n/65535;case Uint8Array:case Uint8ClampedArray:return n/255;case Int32Array:return Math.max(n/2147483647,-1);case Int16Array:return Math.max(n/32767,-1);case Int8Array:return Math.max(n/127,-1);default:throw new Error("THREE.MathUtils: Invalid component type.")}}function ht(n,e){switch(e.constructor){case Float32Array:return n;case Uint32Array:return Math.round(n*4294967295);case Uint16Array:return Math.round(n*65535);case Uint8Array:case Uint8ClampedArray:return Math.round(n*255);case Int32Array:return Math.round(n*2147483647);case Int16Array:return Math.round(n*32767);case Int8Array:return Math.round(n*127);default:throw new Error("THREE.MathUtils: Invalid component type.")}}var Lh={DEG2RAD:rr,RAD2DEG:ds,generateUUID:Ci,clamp:qe,euclideanModulo:Fh,mapLinear:w0,inverseLerp:C0,lerp:so,damp:N0,pingpong:I0,smoothstep:B0,smootherstep:F0,randInt:L0,randFloat:U0,randFloatSpread:D0,seededRandom:P0,degToRad:G0,radToDeg:V0,isPowerOfTwo:O0,ceilPowerOfTwo:k0,floorPowerOfTwo:z0,setQuaternionFromProperEuler:W0,normalize:ht,denormalize:Hn},ze=class n{static{n.prototype.isVector2=!0}constructor(e=0,t=0){this.x=e,this.y=t}get width(){return this.x}set width(e){this.x=e}get height(){return this.y}set height(e){this.y=e}set(e,t){return this.x=e,this.y=t,this}setScalar(e){return this.x=e,this.y=e,this}setX(e){return this.x=e,this}setY(e){return this.y=e,this}setComponent(e,t){switch(e){case 0:this.x=t;break;case 1:this.y=t;break;default:throw new Error("THREE.Vector2: index is out of range: "+e)}return this}getComponent(e){switch(e){case 0:return this.x;case 1:return this.y;default:throw new Error("THREE.Vector2: index is out of range: "+e)}}clone(){return new this.constructor(this.x,this.y)}copy(e){return this.x=e.x,this.y=e.y,this}add(e){return this.x+=e.x,this.y+=e.y,this}addScalar(e){return this.x+=e,this.y+=e,this}addVectors(e,t){return this.x=e.x+t.x,this.y=e.y+t.y,this}addScaledVector(e,t){return this.x+=e.x*t,this.y+=e.y*t,this}sub(e){return this.x-=e.x,this.y-=e.y,this}subScalar(e){return this.x-=e,this.y-=e,this}subVectors(e,t){return this.x=e.x-t.x,this.y=e.y-t.y,this}multiply(e){return this.x*=e.x,this.y*=e.y,this}multiplyScalar(e){return this.x*=e,this.y*=e,this}divide(e){return this.x/=e.x,this.y/=e.y,this}divideScalar(e){return this.multiplyScalar(1/e)}applyMatrix3(e){let t=this.x,i=this.y,s=e.elements;return this.x=s[0]*t+s[3]*i+s[6],this.y=s[1]*t+s[4]*i+s[7],this}min(e){return this.x=Math.min(this.x,e.x),this.y=Math.min(this.y,e.y),this}max(e){return this.x=Math.max(this.x,e.x),this.y=Math.max(this.y,e.y),this}clamp(e,t){return this.x=qe(this.x,e.x,t.x),this.y=qe(this.y,e.y,t.y),this}clampScalar(e,t){return this.x=qe(this.x,e,t),this.y=qe(this.y,e,t),this}clampLength(e,t){let i=this.length();return this.divideScalar(i||1).multiplyScalar(qe(i,e,t))}floor(){return this.x=Math.floor(this.x),this.y=Math.floor(this.y),this}ceil(){return this.x=Math.ceil(this.x),this.y=Math.ceil(this.y),this}round(){return this.x=Math.round(this.x),this.y=Math.round(this.y),this}roundToZero(){return this.x=Math.trunc(this.x),this.y=Math.trunc(this.y),this}negate(){return this.x=-this.x,this.y=-this.y,this}dot(e){return this.x*e.x+this.y*e.y}cross(e){return this.x*e.y-this.y*e.x}lengthSq(){return this.x*this.x+this.y*this.y}length(){return Math.sqrt(this.x*this.x+this.y*this.y)}manhattanLength(){return Math.abs(this.x)+Math.abs(this.y)}normalize(){return this.divideScalar(this.length()||1)}angle(){return Math.atan2(-this.y,-this.x)+Math.PI}angleTo(e){let t=Math.sqrt(this.lengthSq()*e.lengthSq());if(t===0)return Math.PI/2;let i=this.dot(e)/t;return Math.acos(qe(i,-1,1))}distanceTo(e){return Math.sqrt(this.distanceToSquared(e))}distanceToSquared(e){let t=this.x-e.x,i=this.y-e.y;return t*t+i*i}manhattanDistanceTo(e){return Math.abs(this.x-e.x)+Math.abs(this.y-e.y)}setLength(e){return this.normalize().multiplyScalar(e)}lerp(e,t){return this.x+=(e.x-this.x)*t,this.y+=(e.y-this.y)*t,this}lerpVectors(e,t,i){return this.x=e.x+(t.x-e.x)*i,this.y=e.y+(t.y-e.y)*i,this}equals(e){return e.x===this.x&&e.y===this.y}fromArray(e,t=0){return this.x=e[t],this.y=e[t+1],this}toArray(e=[],t=0){return e[t]=this.x,e[t+1]=this.y,e}fromBufferAttribute(e,t){return this.x=e.getX(t),this.y=e.getY(t),this}rotateAround(e,t){let i=Math.cos(t),s=Math.sin(t),r=this.x-e.x,o=this.y-e.y;return this.x=r*i-o*s+e.x,this.y=r*s+o*i+e.y,this}random(){return this.x=Math.random(),this.y=Math.random(),this}*[Symbol.iterator](){yield this.x,yield this.y}},Xn=class{constructor(e=0,t=0,i=0,s=1){this.isQuaternion=!0,this._x=e,this._y=t,this._z=i,this._w=s}static slerpFlat(e,t,i,s,r,o,a){let l=i[s+0],c=i[s+1],u=i[s+2],d=i[s+3],h=r[o+0],f=r[o+1],g=r[o+2],y=r[o+3];if(d!==y||l!==h||c!==f||u!==g){let m=l*h+c*f+u*g+d*y;m<0&&(h=-h,f=-f,g=-g,y=-y,m=-m);let p=1-a;if(m<.9995){let E=Math.acos(m),w=Math.sin(E);p=Math.sin(p*E)/w,a=Math.sin(a*E)/w,l=l*p+h*a,c=c*p+f*a,u=u*p+g*a,d=d*p+y*a}else{l=l*p+h*a,c=c*p+f*a,u=u*p+g*a,d=d*p+y*a;let E=1/Math.sqrt(l*l+c*c+u*u+d*d);l*=E,c*=E,u*=E,d*=E}}e[t]=l,e[t+1]=c,e[t+2]=u,e[t+3]=d}static multiplyQuaternionsFlat(e,t,i,s,r,o){let a=i[s],l=i[s+1],c=i[s+2],u=i[s+3],d=r[o],h=r[o+1],f=r[o+2],g=r[o+3];return e[t]=a*g+u*d+l*f-c*h,e[t+1]=l*g+u*h+c*d-a*f,e[t+2]=c*g+u*f+a*h-l*d,e[t+3]=u*g-a*d-l*h-c*f,e}get x(){return this._x}set x(e){this._x=e,this._onChangeCallback()}get y(){return this._y}set y(e){this._y=e,this._onChangeCallback()}get z(){return this._z}set z(e){this._z=e,this._onChangeCallback()}get w(){return this._w}set w(e){this._w=e,this._onChangeCallback()}set(e,t,i,s){return this._x=e,this._y=t,this._z=i,this._w=s,this._onChangeCallback(),this}clone(){return new this.constructor(this._x,this._y,this._z,this._w)}copy(e){return this._x=e.x,this._y=e.y,this._z=e.z,this._w=e.w,this._onChangeCallback(),this}setFromEuler(e,t=!0){let i=e._x,s=e._y,r=e._z,o=e._order,a=Math.cos,l=Math.sin,c=a(i/2),u=a(s/2),d=a(r/2),h=l(i/2),f=l(s/2),g=l(r/2);switch(o){case"XYZ":this._x=h*u*d+c*f*g,this._y=c*f*d-h*u*g,this._z=c*u*g+h*f*d,this._w=c*u*d-h*f*g;break;case"YXZ":this._x=h*u*d+c*f*g,this._y=c*f*d-h*u*g,this._z=c*u*g-h*f*d,this._w=c*u*d+h*f*g;break;case"ZXY":this._x=h*u*d-c*f*g,this._y=c*f*d+h*u*g,this._z=c*u*g+h*f*d,this._w=c*u*d-h*f*g;break;case"ZYX":this._x=h*u*d-c*f*g,this._y=c*f*d+h*u*g,this._z=c*u*g-h*f*d,this._w=c*u*d+h*f*g;break;case"YZX":this._x=h*u*d+c*f*g,this._y=c*f*d+h*u*g,this._z=c*u*g-h*f*d,this._w=c*u*d-h*f*g;break;case"XZY":this._x=h*u*d-c*f*g,this._y=c*f*d-h*u*g,this._z=c*u*g+h*f*d,this._w=c*u*d+h*f*g;break;default:pe("Quaternion: .setFromEuler() encountered an unknown order: "+o)}return t===!0&&this._onChangeCallback(),this}setFromAxisAngle(e,t){let i=t/2,s=Math.sin(i);return this._x=e.x*s,this._y=e.y*s,this._z=e.z*s,this._w=Math.cos(i),this._onChangeCallback(),this}setFromRotationMatrix(e){let t=e.elements,i=t[0],s=t[4],r=t[8],o=t[1],a=t[5],l=t[9],c=t[2],u=t[6],d=t[10],h=i+a+d;if(h>0){let f=.5/Math.sqrt(h+1);this._w=.25/f,this._x=(u-l)*f,this._y=(r-c)*f,this._z=(o-s)*f}else if(i>a&&i>d){let f=2*Math.sqrt(1+i-a-d);this._w=(u-l)/f,this._x=.25*f,this._y=(s+o)/f,this._z=(r+c)/f}else if(a>d){let f=2*Math.sqrt(1+a-i-d);this._w=(r-c)/f,this._x=(s+o)/f,this._y=.25*f,this._z=(l+u)/f}else{let f=2*Math.sqrt(1+d-i-a);this._w=(o-s)/f,this._x=(r+c)/f,this._y=(l+u)/f,this._z=.25*f}return this._onChangeCallback(),this}setFromUnitVectors(e,t){let i=e.dot(t)+1;return i<1e-8?(i=0,Math.abs(e.x)>Math.abs(e.z)?(this._x=-e.y,this._y=e.x,this._z=0,this._w=i):(this._x=0,this._y=-e.z,this._z=e.y,this._w=i)):(this._x=e.y*t.z-e.z*t.y,this._y=e.z*t.x-e.x*t.z,this._z=e.x*t.y-e.y*t.x,this._w=i),this.normalize()}angleTo(e){return 2*Math.acos(Math.abs(qe(this.dot(e),-1,1)))}rotateTowards(e,t){let i=this.angleTo(e);if(i===0)return this;let s=Math.min(1,t/i);return this.slerp(e,s),this}identity(){return this.set(0,0,0,1)}invert(){return this.conjugate()}conjugate(){return this._x*=-1,this._y*=-1,this._z*=-1,this._onChangeCallback(),this}dot(e){return this._x*e._x+this._y*e._y+this._z*e._z+this._w*e._w}lengthSq(){return this._x*this._x+this._y*this._y+this._z*this._z+this._w*this._w}length(){return Math.sqrt(this._x*this._x+this._y*this._y+this._z*this._z+this._w*this._w)}normalize(){let e=this.length();return e===0?(this._x=0,this._y=0,this._z=0,this._w=1):(e=1/e,this._x=this._x*e,this._y=this._y*e,this._z=this._z*e,this._w=this._w*e),this._onChangeCallback(),this}multiply(e){return this.multiplyQuaternions(this,e)}premultiply(e){return this.multiplyQuaternions(e,this)}multiplyQuaternions(e,t){let i=e._x,s=e._y,r=e._z,o=e._w,a=t._x,l=t._y,c=t._z,u=t._w;return this._x=i*u+o*a+s*c-r*l,this._y=s*u+o*l+r*a-i*c,this._z=r*u+o*c+i*l-s*a,this._w=o*u-i*a-s*l-r*c,this._onChangeCallback(),this}slerp(e,t){let i=e._x,s=e._y,r=e._z,o=e._w,a=this.dot(e);a<0&&(i=-i,s=-s,r=-r,o=-o,a=-a);let l=1-t;if(a<.9995){let c=Math.acos(a),u=Math.sin(c);l=Math.sin(l*c)/u,t=Math.sin(t*c)/u,this._x=this._x*l+i*t,this._y=this._y*l+s*t,this._z=this._z*l+r*t,this._w=this._w*l+o*t,this._onChangeCallback()}else this._x=this._x*l+i*t,this._y=this._y*l+s*t,this._z=this._z*l+r*t,this._w=this._w*l+o*t,this.normalize();return this}slerpQuaternions(e,t,i){return this.copy(e).slerp(t,i)}random(){let e=2*Math.PI*Math.random(),t=2*Math.PI*Math.random(),i=Math.random(),s=Math.sqrt(1-i),r=Math.sqrt(i);return this.set(s*Math.sin(e),s*Math.cos(e),r*Math.sin(t),r*Math.cos(t))}equals(e){return e._x===this._x&&e._y===this._y&&e._z===this._z&&e._w===this._w}fromArray(e,t=0){return this._x=e[t],this._y=e[t+1],this._z=e[t+2],this._w=e[t+3],this._onChangeCallback(),this}toArray(e=[],t=0){return e[t]=this._x,e[t+1]=this._y,e[t+2]=this._z,e[t+3]=this._w,e}fromBufferAttribute(e,t){return this._x=e.getX(t),this._y=e.getY(t),this._z=e.getZ(t),this._w=e.getW(t),this._onChangeCallback(),this}toJSON(){return this.toArray()}_onChange(e){return this._onChangeCallback=e,this}_onChangeCallback(){}*[Symbol.iterator](){yield this._x,yield this._y,yield this._z,yield this._w}},L=class n{static{n.prototype.isVector3=!0}constructor(e=0,t=0,i=0){this.x=e,this.y=t,this.z=i}set(e,t,i){return i===void 0&&(i=this.z),this.x=e,this.y=t,this.z=i,this}setScalar(e){return this.x=e,this.y=e,this.z=e,this}setX(e){return this.x=e,this}setY(e){return this.y=e,this}setZ(e){return this.z=e,this}setComponent(e,t){switch(e){case 0:this.x=t;break;case 1:this.y=t;break;case 2:this.z=t;break;default:throw new Error("THREE.Vector3: index is out of range: "+e)}return this}getComponent(e){switch(e){case 0:return this.x;case 1:return this.y;case 2:return this.z;default:throw new Error("THREE.Vector3: index is out of range: "+e)}}clone(){return new this.constructor(this.x,this.y,this.z)}copy(e){return this.x=e.x,this.y=e.y,this.z=e.z,this}add(e){return this.x+=e.x,this.y+=e.y,this.z+=e.z,this}addScalar(e){return this.x+=e,this.y+=e,this.z+=e,this}addVectors(e,t){return this.x=e.x+t.x,this.y=e.y+t.y,this.z=e.z+t.z,this}addScaledVector(e,t){return this.x+=e.x*t,this.y+=e.y*t,this.z+=e.z*t,this}sub(e){return this.x-=e.x,this.y-=e.y,this.z-=e.z,this}subScalar(e){return this.x-=e,this.y-=e,this.z-=e,this}subVectors(e,t){return this.x=e.x-t.x,this.y=e.y-t.y,this.z=e.z-t.z,this}multiply(e){return this.x*=e.x,this.y*=e.y,this.z*=e.z,this}multiplyScalar(e){return this.x*=e,this.y*=e,this.z*=e,this}multiplyVectors(e,t){return this.x=e.x*t.x,this.y=e.y*t.y,this.z=e.z*t.z,this}applyEuler(e){return this.applyQuaternion(Ep.setFromEuler(e))}applyAxisAngle(e,t){return this.applyQuaternion(Ep.setFromAxisAngle(e,t))}applyMatrix3(e){let t=this.x,i=this.y,s=this.z,r=e.elements;return this.x=r[0]*t+r[3]*i+r[6]*s,this.y=r[1]*t+r[4]*i+r[7]*s,this.z=r[2]*t+r[5]*i+r[8]*s,this}applyNormalMatrix(e){return this.applyMatrix3(e).normalize()}applyMatrix4(e){let t=this.x,i=this.y,s=this.z,r=e.elements,o=1/(r[3]*t+r[7]*i+r[11]*s+r[15]);return this.x=(r[0]*t+r[4]*i+r[8]*s+r[12])*o,this.y=(r[1]*t+r[5]*i+r[9]*s+r[13])*o,this.z=(r[2]*t+r[6]*i+r[10]*s+r[14])*o,this}applyQuaternion(e){let t=this.x,i=this.y,s=this.z,r=e.x,o=e.y,a=e.z,l=e.w,c=2*(o*s-a*i),u=2*(a*t-r*s),d=2*(r*i-o*t);return this.x=t+l*c+o*d-a*u,this.y=i+l*u+a*c-r*d,this.z=s+l*d+r*u-o*c,this}project(e){return this.applyMatrix4(e.matrixWorldInverse).applyMatrix4(e.projectionMatrix)}unproject(e){return this.applyMatrix4(e.projectionMatrixInverse).applyMatrix4(e.matrixWorld)}transformDirection(e){let t=this.x,i=this.y,s=this.z,r=e.elements;return this.x=r[0]*t+r[4]*i+r[8]*s,this.y=r[1]*t+r[5]*i+r[9]*s,this.z=r[2]*t+r[6]*i+r[10]*s,this.normalize()}divide(e){return this.x/=e.x,this.y/=e.y,this.z/=e.z,this}divideScalar(e){return this.multiplyScalar(1/e)}min(e){return this.x=Math.min(this.x,e.x),this.y=Math.min(this.y,e.y),this.z=Math.min(this.z,e.z),this}max(e){return this.x=Math.max(this.x,e.x),this.y=Math.max(this.y,e.y),this.z=Math.max(this.z,e.z),this}clamp(e,t){return this.x=qe(this.x,e.x,t.x),this.y=qe(this.y,e.y,t.y),this.z=qe(this.z,e.z,t.z),this}clampScalar(e,t){return this.x=qe(this.x,e,t),this.y=qe(this.y,e,t),this.z=qe(this.z,e,t),this}clampLength(e,t){let i=this.length();return this.divideScalar(i||1).multiplyScalar(qe(i,e,t))}floor(){return this.x=Math.floor(this.x),this.y=Math.floor(this.y),this.z=Math.floor(this.z),this}ceil(){return this.x=Math.ceil(this.x),this.y=Math.ceil(this.y),this.z=Math.ceil(this.z),this}round(){return this.x=Math.round(this.x),this.y=Math.round(this.y),this.z=Math.round(this.z),this}roundToZero(){return this.x=Math.trunc(this.x),this.y=Math.trunc(this.y),this.z=Math.trunc(this.z),this}negate(){return this.x=-this.x,this.y=-this.y,this.z=-this.z,this}dot(e){return this.x*e.x+this.y*e.y+this.z*e.z}lengthSq(){return this.x*this.x+this.y*this.y+this.z*this.z}length(){return Math.sqrt(this.x*this.x+this.y*this.y+this.z*this.z)}manhattanLength(){return Math.abs(this.x)+Math.abs(this.y)+Math.abs(this.z)}normalize(){return this.divideScalar(this.length()||1)}setLength(e){return this.normalize().multiplyScalar(e)}lerp(e,t){return this.x+=(e.x-this.x)*t,this.y+=(e.y-this.y)*t,this.z+=(e.z-this.z)*t,this}lerpVectors(e,t,i){return this.x=e.x+(t.x-e.x)*i,this.y=e.y+(t.y-e.y)*i,this.z=e.z+(t.z-e.z)*i,this}cross(e){return this.crossVectors(this,e)}crossVectors(e,t){let i=e.x,s=e.y,r=e.z,o=t.x,a=t.y,l=t.z;return this.x=s*l-r*a,this.y=r*o-i*l,this.z=i*a-s*o,this}projectOnVector(e){let t=e.lengthSq();if(t===0)return this.set(0,0,0);let i=e.dot(this)/t;return this.copy(e).multiplyScalar(i)}projectOnPlane(e){return Eu.copy(this).projectOnVector(e),this.sub(Eu)}reflect(e){return this.sub(Eu.copy(e).multiplyScalar(2*this.dot(e)))}angleTo(e){let t=Math.sqrt(this.lengthSq()*e.lengthSq());if(t===0)return Math.PI/2;let i=this.dot(e)/t;return Math.acos(qe(i,-1,1))}distanceTo(e){return Math.sqrt(this.distanceToSquared(e))}distanceToSquared(e){let t=this.x-e.x,i=this.y-e.y,s=this.z-e.z;return t*t+i*i+s*s}manhattanDistanceTo(e){return Math.abs(this.x-e.x)+Math.abs(this.y-e.y)+Math.abs(this.z-e.z)}setFromSpherical(e){return this.setFromSphericalCoords(e.radius,e.phi,e.theta)}setFromSphericalCoords(e,t,i){let s=Math.sin(t)*e;return this.x=s*Math.sin(i),this.y=Math.cos(t)*e,this.z=s*Math.cos(i),this}setFromCylindrical(e){return this.setFromCylindricalCoords(e.radius,e.theta,e.y)}setFromCylindricalCoords(e,t,i){return this.x=e*Math.sin(t),this.y=i,this.z=e*Math.cos(t),this}setFromMatrixPosition(e){let t=e.elements;return this.x=t[12],this.y=t[13],this.z=t[14],this}setFromMatrixScale(e){let t=this.setFromMatrixColumn(e,0).length(),i=this.setFromMatrixColumn(e,1).length(),s=this.setFromMatrixColumn(e,2).length();return this.x=t,this.y=i,this.z=s,this}setFromMatrixColumn(e,t){return this.fromArray(e.elements,t*4)}setFromMatrix3Column(e,t){return this.fromArray(e.elements,t*3)}setFromEuler(e){return this.x=e._x,this.y=e._y,this.z=e._z,this}setFromColor(e){return this.x=e.r,this.y=e.g,this.z=e.b,this}equals(e){return e.x===this.x&&e.y===this.y&&e.z===this.z}fromArray(e,t=0){return this.x=e[t],this.y=e[t+1],this.z=e[t+2],this}toArray(e=[],t=0){return e[t]=this.x,e[t+1]=this.y,e[t+2]=this.z,e}fromBufferAttribute(e,t){return this.x=e.getX(t),this.y=e.getY(t),this.z=e.getZ(t),this}random(){return this.x=Math.random(),this.y=Math.random(),this.z=Math.random(),this}randomDirection(){let e=Math.random()*Math.PI*2,t=Math.random()*2-1,i=Math.sqrt(1-t*t);return this.x=i*Math.cos(e),this.y=t,this.z=i*Math.sin(e),this}*[Symbol.iterator](){yield this.x,yield this.y,yield this.z}},Eu=new L,Ep=new Xn,Ge=class n{static{n.prototype.isMatrix3=!0}constructor(e,t,i,s,r,o,a,l,c){this.elements=[1,0,0,0,1,0,0,0,1],e!==void 0&&this.set(e,t,i,s,r,o,a,l,c)}set(e,t,i,s,r,o,a,l,c){let u=this.elements;return u[0]=e,u[1]=s,u[2]=a,u[3]=t,u[4]=r,u[5]=l,u[6]=i,u[7]=o,u[8]=c,this}identity(){return this.set(1,0,0,0,1,0,0,0,1),this}copy(e){let t=this.elements,i=e.elements;return t[0]=i[0],t[1]=i[1],t[2]=i[2],t[3]=i[3],t[4]=i[4],t[5]=i[5],t[6]=i[6],t[7]=i[7],t[8]=i[8],this}extractBasis(e,t,i){return e.setFromMatrix3Column(this,0),t.setFromMatrix3Column(this,1),i.setFromMatrix3Column(this,2),this}setFromMatrix4(e){let t=e.elements;return this.set(t[0],t[4],t[8],t[1],t[5],t[9],t[2],t[6],t[10]),this}multiply(e){return this.multiplyMatrices(this,e)}premultiply(e){return this.multiplyMatrices(e,this)}multiplyMatrices(e,t){let i=e.elements,s=t.elements,r=this.elements,o=i[0],a=i[3],l=i[6],c=i[1],u=i[4],d=i[7],h=i[2],f=i[5],g=i[8],y=s[0],m=s[3],p=s[6],E=s[1],w=s[4],S=s[7],v=s[2],A=s[5],R=s[8];return r[0]=o*y+a*E+l*v,r[3]=o*m+a*w+l*A,r[6]=o*p+a*S+l*R,r[1]=c*y+u*E+d*v,r[4]=c*m+u*w+d*A,r[7]=c*p+u*S+d*R,r[2]=h*y+f*E+g*v,r[5]=h*m+f*w+g*A,r[8]=h*p+f*S+g*R,this}multiplyScalar(e){let t=this.elements;return t[0]*=e,t[3]*=e,t[6]*=e,t[1]*=e,t[4]*=e,t[7]*=e,t[2]*=e,t[5]*=e,t[8]*=e,this}determinant(){let e=this.elements,t=e[0],i=e[1],s=e[2],r=e[3],o=e[4],a=e[5],l=e[6],c=e[7],u=e[8];return t*o*u-t*a*c-i*r*u+i*a*l+s*r*c-s*o*l}invert(){let e=this.elements,t=e[0],i=e[1],s=e[2],r=e[3],o=e[4],a=e[5],l=e[6],c=e[7],u=e[8],d=u*o-a*c,h=a*l-u*r,f=c*r-o*l,g=t*d+i*h+s*f;if(g===0)return this.set(0,0,0,0,0,0,0,0,0);let y=1/g;return e[0]=d*y,e[1]=(s*c-u*i)*y,e[2]=(a*i-s*o)*y,e[3]=h*y,e[4]=(u*t-s*l)*y,e[5]=(s*r-a*t)*y,e[6]=f*y,e[7]=(i*l-c*t)*y,e[8]=(o*t-i*r)*y,this}transpose(){let e,t=this.elements;return e=t[1],t[1]=t[3],t[3]=e,e=t[2],t[2]=t[6],t[6]=e,e=t[5],t[5]=t[7],t[7]=e,this}getNormalMatrix(e){return this.setFromMatrix4(e).invert().transpose()}transposeIntoArray(e){let t=this.elements;return e[0]=t[0],e[1]=t[3],e[2]=t[6],e[3]=t[1],e[4]=t[4],e[5]=t[7],e[6]=t[2],e[7]=t[5],e[8]=t[8],this}setUvTransform(e,t,i,s,r,o,a){let l=Math.cos(r),c=Math.sin(r);return this.set(i*l,i*c,-i*(l*o+c*a)+o+e,-s*c,s*l,-s*(-c*o+l*a)+a+t,0,0,1),this}scale(e,t){return Zn("Matrix3: .scale() is deprecated. Use .makeScale() instead."),this.premultiply(Mu.makeScale(e,t)),this}rotate(e){return Zn("Matrix3: .rotate() is deprecated. Use .makeRotation() instead."),this.premultiply(Mu.makeRotation(-e)),this}translate(e,t){return Zn("Matrix3: .translate() is deprecated. Use .makeTranslation() instead."),this.premultiply(Mu.makeTranslation(e,t)),this}makeTranslation(e,t){return e.isVector2?this.set(1,0,e.x,0,1,e.y,0,0,1):this.set(1,0,e,0,1,t,0,0,1),this}makeRotation(e){let t=Math.cos(e),i=Math.sin(e);return this.set(t,-i,0,i,t,0,0,0,1),this}makeScale(e,t){return this.set(e,0,0,0,t,0,0,0,1),this}equals(e){let t=this.elements,i=e.elements;for(let s=0;s<9;s++)if(t[s]!==i[s])return!1;return!0}fromArray(e,t=0){for(let i=0;i<9;i++)this.elements[i]=e[i+t];return this}toArray(e=[],t=0){let i=this.elements;return e[t]=i[0],e[t+1]=i[1],e[t+2]=i[2],e[t+3]=i[3],e[t+4]=i[4],e[t+5]=i[5],e[t+6]=i[6],e[t+7]=i[7],e[t+8]=i[8],e}clone(){return new this.constructor().fromArray(this.elements)}},Mu=new Ge,Mp=new Ge().set(.4123908,.3575843,.1804808,.212639,.7151687,.0721923,.0193308,.1191948,.9505322),Rp=new Ge().set(3.2409699,-1.5373832,-.4986108,-.9692436,1.8759675,.0415551,.0556301,-.203977,1.0569715);function H0(){let n={enabled:!0,workingColorSpace:Wi,spaces:{},convert:function(s,r,o){return this.enabled===!1||r===o||!r||!o||(this.spaces[r].transfer===nt&&(s.r=Ni(s.r),s.g=Ni(s.g),s.b=Ni(s.b)),this.spaces[r].primaries!==this.spaces[o].primaries&&(s.applyMatrix3(this.spaces[r].toXYZ),s.applyMatrix3(this.spaces[o].fromXYZ)),this.spaces[o].transfer===nt&&(s.r=or(s.r),s.g=or(s.g),s.b=or(s.b))),s},workingToColorSpace:function(s,r){return this.convert(s,this.workingColorSpace,r)},colorSpaceToWorking:function(s,r){return this.convert(s,r,this.workingColorSpace)},getPrimaries:function(s){return this.spaces[s].primaries},getTransfer:function(s){return s===Pn?cr:this.spaces[s].transfer},getToneMappingMode:function(s){return this.spaces[s].outputColorSpaceConfig.toneMappingMode||"standard"},getLuminanceCoefficients:function(s,r=this.workingColorSpace){return s.fromArray(this.spaces[r].luminanceCoefficients)},define:function(s){Object.assign(this.spaces,s)},_getMatrix:function(s,r,o){return s.copy(this.spaces[r].toXYZ).multiply(this.spaces[o].fromXYZ)},_getDrawingBufferColorSpace:function(s){return this.spaces[s].outputColorSpaceConfig.drawingBufferColorSpace},_getUnpackColorSpace:function(s=this.workingColorSpace){return this.spaces[s].workingColorSpaceConfig.unpackColorSpace},fromWorkingColorSpace:function(s,r){return Zn("ColorManagement: .fromWorkingColorSpace() has been renamed to .workingToColorSpace()."),n.workingToColorSpace(s,r)},toWorkingColorSpace:function(s,r){return Zn("ColorManagement: .toWorkingColorSpace() has been renamed to .colorSpaceToWorking()."),n.colorSpaceToWorking(s,r)}},e=[.64,.33,.3,.6,.15,.06],t=[.2126,.7152,.0722],i=[.3127,.329];return n.define({[Wi]:{primaries:e,whitePoint:i,transfer:cr,toXYZ:Mp,fromXYZ:Rp,luminanceCoefficients:t,workingColorSpaceConfig:{unpackColorSpace:yn},outputColorSpaceConfig:{drawingBufferColorSpace:yn}},[yn]:{primaries:e,whitePoint:i,transfer:nt,toXYZ:Mp,fromXYZ:Rp,luminanceCoefficients:t,outputColorSpaceConfig:{drawingBufferColorSpace:yn}}}),n}var Ve=H0();function Ni(n){return n<.04045?n*.0773993808:Math.pow(n*.9478672986+.0521327014,2.4)}function or(n){return n<.0031308?n*12.92:1.055*Math.pow(n,.41666)-.055}var Zs,Tl=class{static getDataURL(e,t="image/png"){if(/^data:/i.test(e.src)||typeof HTMLCanvasElement>"u")return e.src;let i;if(e instanceof HTMLCanvasElement)i=e;else{Zs===void 0&&(Zs=po("canvas")),Zs.width=e.width,Zs.height=e.height;let s=Zs.getContext("2d");e instanceof ImageData?s.putImageData(e,0,0):s.drawImage(e,0,0,e.width,e.height),i=Zs}return i.toDataURL(t)}static sRGBToLinear(e){if(typeof HTMLImageElement<"u"&&e instanceof HTMLImageElement||typeof HTMLCanvasElement<"u"&&e instanceof HTMLCanvasElement||typeof ImageBitmap<"u"&&e instanceof ImageBitmap){let t=po("canvas");t.width=e.width,t.height=e.height;let i=t.getContext("2d");i.drawImage(e,0,0,e.width,e.height);let s=i.getImageData(0,0,e.width,e.height),r=s.data;for(let o=0;o<r.length;o++)r[o]=Ni(r[o]/255)*255;return i.putImageData(s,0,0),t}else if(e.data){let t=e.data.slice(0);for(let i=0;i<t.length;i++)t instanceof Uint8Array||t instanceof Uint8ClampedArray?t[i]=Math.floor(Ni(t[i]/255)*255):t[i]=Ni(t[i]);return{data:t,width:e.width,height:e.height}}else return pe("ImageUtils.sRGBToLinear(): Unsupported image type. No color space conversion applied."),e}},Y0=0,hr=class{constructor(e=null){this.isTextureSource=!0,Object.defineProperty(this,"id",{value:Y0++}),this.uuid=Ci(),this.data=e,this.dataReady=!0,this.version=0}getSize(e){let t=this.data;return typeof HTMLVideoElement<"u"&&t instanceof HTMLVideoElement?e.set(t.videoWidth,t.videoHeight,0):typeof VideoFrame<"u"&&t instanceof VideoFrame?e.set(t.displayWidth,t.displayHeight,0):t!==null?e.set(t.width,t.height,t.depth||0):e.set(0,0,0),e}set needsUpdate(e){e===!0&&this.version++}toJSON(e){let t=e===void 0||typeof e=="string";if(!t&&e.images[this.uuid]!==void 0)return e.images[this.uuid];let i={uuid:this.uuid,url:""},s=this.data;if(s!==null){let r;if(Array.isArray(s)){r=[];for(let o=0,a=s.length;o<a;o++)s[o].isDataTexture?r.push(Ru(s[o].image)):r.push(Ru(s[o]))}else r=Ru(s);i.url=r}return t||(e.images[this.uuid]=i),i}};function Ru(n){return typeof HTMLImageElement<"u"&&n instanceof HTMLImageElement||typeof HTMLCanvasElement<"u"&&n instanceof HTMLCanvasElement||typeof ImageBitmap<"u"&&n instanceof ImageBitmap?Tl.getDataURL(n):n.data?{data:Array.from(n.data),width:n.width,height:n.height,type:n.data.constructor.name}:(pe("Texture: Unable to serialize Texture."),{})}var Z0=0,wu=new L,$t=class n extends wn{constructor(e=n.DEFAULT_IMAGE,t=n.DEFAULT_MAPPING,i=Ln,s=Ln,r=Bt,o=di,a=gn,l=mn,c=n.DEFAULT_ANISOTROPY,u=Pn){super(),this.isTexture=!0,Object.defineProperty(this,"id",{value:Z0++}),this.uuid=Ci(),this.name="",this.source=new hr(e),this.mipmaps=[],this.mapping=t,this.channel=0,this.wrapS=i,this.wrapT=s,this.magFilter=r,this.minFilter=o,this.anisotropy=c,this.format=a,this.internalFormat=null,this.type=l,this.offset=new ze(0,0),this.repeat=new ze(1,1),this.center=new ze(0,0),this.rotation=0,this.matrixAutoUpdate=!0,this.matrix=new Ge,this.generateMipmaps=!0,this.premultiplyAlpha=!1,this.flipY=!0,this.unpackAlignment=4,this.colorSpace=u,this.userData={},this.updateRanges=[],this.version=0,this.onUpdate=null,this.renderTarget=null,this.isRenderTargetTexture=!1,this.isArrayTexture=!!(e&&e.depth&&e.depth>1),this.pmremVersion=0,this.normalized=!1}get width(){return this.source.getSize(wu).x}get height(){return this.source.getSize(wu).y}get depth(){return this.source.getSize(wu).z}get image(){return this.source.data}set image(e){this.source.data=e}updateMatrix(){this.matrix.setUvTransform(this.offset.x,this.offset.y,this.repeat.x,this.repeat.y,this.rotation,this.center.x,this.center.y)}addUpdateRange(e,t){this.updateRanges.push({start:e,count:t})}clearUpdateRanges(){this.updateRanges.length=0}clone(){return new this.constructor().copy(this)}copy(e){return this.name=e.name,this.source=e.source,this.mipmaps=e.mipmaps.slice(0),this.mapping=e.mapping,this.channel=e.channel,this.wrapS=e.wrapS,this.wrapT=e.wrapT,this.magFilter=e.magFilter,this.minFilter=e.minFilter,this.anisotropy=e.anisotropy,this.format=e.format,this.internalFormat=e.internalFormat,this.type=e.type,this.normalized=e.normalized,this.offset.copy(e.offset),this.repeat.copy(e.repeat),this.center.copy(e.center),this.rotation=e.rotation,this.matrixAutoUpdate=e.matrixAutoUpdate,this.matrix.copy(e.matrix),this.generateMipmaps=e.generateMipmaps,this.premultiplyAlpha=e.premultiplyAlpha,this.flipY=e.flipY,this.unpackAlignment=e.unpackAlignment,this.colorSpace=e.colorSpace,this.renderTarget=e.renderTarget,this.isRenderTargetTexture=e.isRenderTargetTexture,this.isArrayTexture=e.isArrayTexture,this.userData=JSON.parse(JSON.stringify(e.userData)),this.needsUpdate=!0,this}setValues(e){for(let t in e){let i=e[t];if(i===void 0){pe(`Texture.setValues(): parameter '${t}' has value of undefined.`);continue}let s=this[t];if(s===void 0){pe(`Texture.setValues(): property '${t}' does not exist.`);continue}s&&i&&s.isVector2&&i.isVector2||s&&i&&s.isVector3&&i.isVector3||s&&i&&s.isMatrix3&&i.isMatrix3?s.copy(i):this[t]=i}}toJSON(e){let t=e===void 0||typeof e=="string";if(!t&&e.textures[this.uuid]!==void 0)return e.textures[this.uuid];let i={metadata:{version:4.7,type:"Texture",generator:"Texture.toJSON"},uuid:this.uuid,name:this.name,image:this.source.toJSON(e).uuid,mapping:this.mapping,channel:this.channel,repeat:[this.repeat.x,this.repeat.y],offset:[this.offset.x,this.offset.y],center:[this.center.x,this.center.y],rotation:this.rotation,wrap:[this.wrapS,this.wrapT],format:this.format,internalFormat:this.internalFormat,type:this.type,normalized:this.normalized,colorSpace:this.colorSpace,minFilter:this.minFilter,magFilter:this.magFilter,anisotropy:this.anisotropy,flipY:this.flipY,generateMipmaps:this.generateMipmaps,premultiplyAlpha:this.premultiplyAlpha,unpackAlignment:this.unpackAlignment};return Object.keys(this.userData).length>0&&(i.userData=this.userData),t||(e.textures[this.uuid]=i),i}dispose(){this.dispatchEvent({type:"dispose"})}transformUv(e){if(this.mapping!==tc)return e;if(e.applyMatrix3(this.matrix),e.x<0||e.x>1)switch(this.wrapS){case ar:e.x=e.x-Math.floor(e.x);break;case Ln:e.x=e.x<0?0:1;break;case lr:Math.abs(Math.floor(e.x)%2)===1?e.x=Math.ceil(e.x)-e.x:e.x=e.x-Math.floor(e.x);break}if(e.y<0||e.y>1)switch(this.wrapT){case ar:e.y=e.y-Math.floor(e.y);break;case Ln:e.y=e.y<0?0:1;break;case lr:Math.abs(Math.floor(e.y)%2)===1?e.y=Math.ceil(e.y)-e.y:e.y=e.y-Math.floor(e.y);break}return this.flipY&&(e.y=1-e.y),e}set needsUpdate(e){e===!0&&(this.version++,this.source.needsUpdate=!0)}set needsPMREMUpdate(e){e===!0&&this.pmremVersion++}};$t.DEFAULT_IMAGE=null;$t.DEFAULT_MAPPING=tc;$t.DEFAULT_ANISOTROPY=1;var dt=class n{static{n.prototype.isVector4=!0}constructor(e=0,t=0,i=0,s=1){this.x=e,this.y=t,this.z=i,this.w=s}get width(){return this.z}set width(e){this.z=e}get height(){return this.w}set height(e){this.w=e}set(e,t,i,s){return this.x=e,this.y=t,this.z=i,this.w=s,this}setScalar(e){return this.x=e,this.y=e,this.z=e,this.w=e,this}setX(e){return this.x=e,this}setY(e){return this.y=e,this}setZ(e){return this.z=e,this}setW(e){return this.w=e,this}setComponent(e,t){switch(e){case 0:this.x=t;break;case 1:this.y=t;break;case 2:this.z=t;break;case 3:this.w=t;break;default:throw new Error("THREE.Vector4: index is out of range: "+e)}return this}getComponent(e){switch(e){case 0:return this.x;case 1:return this.y;case 2:return this.z;case 3:return this.w;default:throw new Error("THREE.Vector4: index is out of range: "+e)}}clone(){return new this.constructor(this.x,this.y,this.z,this.w)}copy(e){return this.x=e.x,this.y=e.y,this.z=e.z,this.w=e.w!==void 0?e.w:1,this}add(e){return this.x+=e.x,this.y+=e.y,this.z+=e.z,this.w+=e.w,this}addScalar(e){return this.x+=e,this.y+=e,this.z+=e,this.w+=e,this}addVectors(e,t){return this.x=e.x+t.x,this.y=e.y+t.y,this.z=e.z+t.z,this.w=e.w+t.w,this}addScaledVector(e,t){return this.x+=e.x*t,this.y+=e.y*t,this.z+=e.z*t,this.w+=e.w*t,this}sub(e){return this.x-=e.x,this.y-=e.y,this.z-=e.z,this.w-=e.w,this}subScalar(e){return this.x-=e,this.y-=e,this.z-=e,this.w-=e,this}subVectors(e,t){return this.x=e.x-t.x,this.y=e.y-t.y,this.z=e.z-t.z,this.w=e.w-t.w,this}multiply(e){return this.x*=e.x,this.y*=e.y,this.z*=e.z,this.w*=e.w,this}multiplyScalar(e){return this.x*=e,this.y*=e,this.z*=e,this.w*=e,this}applyMatrix4(e){let t=this.x,i=this.y,s=this.z,r=this.w,o=e.elements;return this.x=o[0]*t+o[4]*i+o[8]*s+o[12]*r,this.y=o[1]*t+o[5]*i+o[9]*s+o[13]*r,this.z=o[2]*t+o[6]*i+o[10]*s+o[14]*r,this.w=o[3]*t+o[7]*i+o[11]*s+o[15]*r,this}divide(e){return this.x/=e.x,this.y/=e.y,this.z/=e.z,this.w/=e.w,this}divideScalar(e){return this.multiplyScalar(1/e)}setAxisAngleFromQuaternion(e){this.w=2*Math.acos(e.w);let t=Math.sqrt(1-e.w*e.w);return t<1e-4?(this.x=1,this.y=0,this.z=0):(this.x=e.x/t,this.y=e.y/t,this.z=e.z/t),this}setAxisAngleFromRotationMatrix(e){let t,i,s,r,l=e.elements,c=l[0],u=l[4],d=l[8],h=l[1],f=l[5],g=l[9],y=l[2],m=l[6],p=l[10];if(Math.abs(u-h)<.01&&Math.abs(d-y)<.01&&Math.abs(g-m)<.01){if(Math.abs(u+h)<.1&&Math.abs(d+y)<.1&&Math.abs(g+m)<.1&&Math.abs(c+f+p-3)<.1)return this.set(1,0,0,0),this;t=Math.PI;let w=(c+1)/2,S=(f+1)/2,v=(p+1)/2,A=(u+h)/4,R=(d+y)/4,b=(g+m)/4;return w>S&&w>v?w<.01?(i=0,s=.707106781,r=.707106781):(i=Math.sqrt(w),s=A/i,r=R/i):S>v?S<.01?(i=.707106781,s=0,r=.707106781):(s=Math.sqrt(S),i=A/s,r=b/s):v<.01?(i=.707106781,s=.707106781,r=0):(r=Math.sqrt(v),i=R/r,s=b/r),this.set(i,s,r,t),this}let E=Math.sqrt((m-g)*(m-g)+(d-y)*(d-y)+(h-u)*(h-u));return Math.abs(E)<.001&&(E=1),this.x=(m-g)/E,this.y=(d-y)/E,this.z=(h-u)/E,this.w=Math.acos((c+f+p-1)/2),this}setFromMatrixPosition(e){let t=e.elements;return this.x=t[12],this.y=t[13],this.z=t[14],this.w=t[15],this}min(e){return this.x=Math.min(this.x,e.x),this.y=Math.min(this.y,e.y),this.z=Math.min(this.z,e.z),this.w=Math.min(this.w,e.w),this}max(e){return this.x=Math.max(this.x,e.x),this.y=Math.max(this.y,e.y),this.z=Math.max(this.z,e.z),this.w=Math.max(this.w,e.w),this}clamp(e,t){return this.x=qe(this.x,e.x,t.x),this.y=qe(this.y,e.y,t.y),this.z=qe(this.z,e.z,t.z),this.w=qe(this.w,e.w,t.w),this}clampScalar(e,t){return this.x=qe(this.x,e,t),this.y=qe(this.y,e,t),this.z=qe(this.z,e,t),this.w=qe(this.w,e,t),this}clampLength(e,t){let i=this.length();return this.divideScalar(i||1).multiplyScalar(qe(i,e,t))}floor(){return this.x=Math.floor(this.x),this.y=Math.floor(this.y),this.z=Math.floor(this.z),this.w=Math.floor(this.w),this}ceil(){return this.x=Math.ceil(this.x),this.y=Math.ceil(this.y),this.z=Math.ceil(this.z),this.w=Math.ceil(this.w),this}round(){return this.x=Math.round(this.x),this.y=Math.round(this.y),this.z=Math.round(this.z),this.w=Math.round(this.w),this}roundToZero(){return this.x=Math.trunc(this.x),this.y=Math.trunc(this.y),this.z=Math.trunc(this.z),this.w=Math.trunc(this.w),this}negate(){return this.x=-this.x,this.y=-this.y,this.z=-this.z,this.w=-this.w,this}dot(e){return this.x*e.x+this.y*e.y+this.z*e.z+this.w*e.w}lengthSq(){return this.x*this.x+this.y*this.y+this.z*this.z+this.w*this.w}length(){return Math.sqrt(this.x*this.x+this.y*this.y+this.z*this.z+this.w*this.w)}manhattanLength(){return Math.abs(this.x)+Math.abs(this.y)+Math.abs(this.z)+Math.abs(this.w)}normalize(){return this.divideScalar(this.length()||1)}setLength(e){return this.normalize().multiplyScalar(e)}lerp(e,t){return this.x+=(e.x-this.x)*t,this.y+=(e.y-this.y)*t,this.z+=(e.z-this.z)*t,this.w+=(e.w-this.w)*t,this}lerpVectors(e,t,i){return this.x=e.x+(t.x-e.x)*i,this.y=e.y+(t.y-e.y)*i,this.z=e.z+(t.z-e.z)*i,this.w=e.w+(t.w-e.w)*i,this}equals(e){return e.x===this.x&&e.y===this.y&&e.z===this.z&&e.w===this.w}fromArray(e,t=0){return this.x=e[t],this.y=e[t+1],this.z=e[t+2],this.w=e[t+3],this}toArray(e=[],t=0){return e[t]=this.x,e[t+1]=this.y,e[t+2]=this.z,e[t+3]=this.w,e}fromBufferAttribute(e,t){return this.x=e.getX(t),this.y=e.getY(t),this.z=e.getZ(t),this.w=e.getW(t),this}random(){return this.x=Math.random(),this.y=Math.random(),this.z=Math.random(),this.w=Math.random(),this}*[Symbol.iterator](){yield this.x,yield this.y,yield this.z,yield this.w}},dr=class extends wn{constructor(e=1,t=1,i={}){super(),i=Object.assign({generateMipmaps:!1,internalFormat:null,minFilter:Bt,depthBuffer:!0,stencilBuffer:!1,resolveColorBuffer:!0,resolveDepthBuffer:!0,resolveStencilBuffer:!0,storeMultisampledColorBuffer:!0,storeMultisampledDepthBuffer:!0,storeMultisampledStencilBuffer:!0,depthTexture:null,samples:0,count:1,depth:1,multiview:!1,useArrayDepthTexture:!1},i),this.isRenderTarget=!0,this.width=e,this.height=t,this.depth=i.depth,this.scissor=new dt(0,0,e,t),this.scissorTest=!1,this.viewport=new dt(0,0,e,t),this.textures=[];let s={width:e,height:t,depth:i.depth},r=new $t(s),o=i.count;for(let a=0;a<o;a++)this.textures[a]=r.clone(),this.textures[a].isRenderTargetTexture=!0,this.textures[a].renderTarget=this;this._setTextureOptions(i),this.depthBuffer=i.depthBuffer,this.stencilBuffer=i.stencilBuffer,this.resolveColorBuffer=i.resolveColorBuffer,this.resolveDepthBuffer=i.resolveDepthBuffer,this.resolveStencilBuffer=i.resolveStencilBuffer,this.storeMultisampledColorBuffer=i.storeMultisampledColorBuffer,this.storeMultisampledDepthBuffer=i.storeMultisampledDepthBuffer,this.storeMultisampledStencilBuffer=i.storeMultisampledStencilBuffer,this._depthTexture=null,this.depthTexture=i.depthTexture,this.samples=i.samples,this.multiview=i.multiview,this.useArrayDepthTexture=i.useArrayDepthTexture}_setTextureOptions(e={}){let t={minFilter:Bt,generateMipmaps:!1,flipY:!1,internalFormat:null};e.mapping!==void 0&&(t.mapping=e.mapping),e.wrapS!==void 0&&(t.wrapS=e.wrapS),e.wrapT!==void 0&&(t.wrapT=e.wrapT),e.wrapR!==void 0&&(t.wrapR=e.wrapR),e.magFilter!==void 0&&(t.magFilter=e.magFilter),e.minFilter!==void 0&&(t.minFilter=e.minFilter),e.format!==void 0&&(t.format=e.format),e.type!==void 0&&(t.type=e.type),e.anisotropy!==void 0&&(t.anisotropy=e.anisotropy),e.colorSpace!==void 0&&(t.colorSpace=e.colorSpace),e.flipY!==void 0&&(t.flipY=e.flipY),e.generateMipmaps!==void 0&&(t.generateMipmaps=e.generateMipmaps),e.internalFormat!==void 0&&(t.internalFormat=e.internalFormat);for(let i=0;i<this.textures.length;i++)this.textures[i].setValues(t)}get texture(){return this.textures[0]}set texture(e){this.textures[0]=e}set depthTexture(e){this._depthTexture!==null&&this._depthTexture.renderTarget===this&&(this._depthTexture.renderTarget=null),e!==null&&e.renderTarget===null&&(e.renderTarget=this),this._depthTexture=e}get depthTexture(){return this._depthTexture}setSize(e,t,i=1){if(this.width!==e||this.height!==t||this.depth!==i){this.width=e,this.height=t,this.depth=i;for(let s=0,r=this.textures.length;s<r;s++)this.textures[s].image.width=e,this.textures[s].image.height=t,this.textures[s].image.depth=i,this.textures[s].isData3DTexture!==!0&&(this.textures[s].isArrayTexture=this.textures[s].image.depth>1);this.dispose()}this.viewport.set(0,0,e,t),this.scissor.set(0,0,e,t)}clone(){return new this.constructor().copy(this)}copy(e){this.width=e.width,this.height=e.height,this.depth=e.depth,this.scissor.copy(e.scissor),this.scissorTest=e.scissorTest,this.viewport.copy(e.viewport),this.textures.length=0;for(let t=0,i=e.textures.length;t<i;t++){this.textures[t]=e.textures[t].clone(),this.textures[t].isRenderTargetTexture=!0,this.textures[t].renderTarget=this;let s=Object.assign({},e.textures[t].image);this.textures[t].source=new hr(s)}if(this.depthBuffer=e.depthBuffer,this.stencilBuffer=e.stencilBuffer,this.resolveColorBuffer=e.resolveColorBuffer,this.resolveDepthBuffer=e.resolveDepthBuffer,this.resolveStencilBuffer=e.resolveStencilBuffer,this.storeMultisampledColorBuffer=e.storeMultisampledColorBuffer,this.storeMultisampledDepthBuffer=e.storeMultisampledDepthBuffer,this.storeMultisampledStencilBuffer=e.storeMultisampledStencilBuffer,e.depthTexture!==null)if(e.depthTexture.renderTarget===e){let t=e.depthTexture.clone();t.renderTarget=null,this.depthTexture=t}else this.depthTexture=e.depthTexture;return this.samples=e.samples,this.multiview=e.multiview,this.useArrayDepthTexture=e.useArrayDepthTexture,this}dispose(){this.dispatchEvent({type:"dispose"})}},_n=class extends dr{constructor(e=1,t=1,i={}){super(e,t,i),this.isWebGLRenderTarget=!0}},fr=class extends $t{constructor(e=null,t=1,i=1,s=1){super(null),this.isDataArrayTexture=!0,this.image={data:e,width:t,height:i,depth:s},this.magFilter=Rt,this.minFilter=Rt,this.wrapR=Ln,this.generateMipmaps=!1,this.flipY=!1,this.unpackAlignment=1,this.layerUpdates=new Set}copy(e){return super.copy(e),this.wrapR=e.wrapR,this}addLayerUpdate(e){this.layerUpdates.add(e)}clearLayerUpdates(){this.layerUpdates.clear()}};var El=class extends $t{constructor(e=null,t=1,i=1,s=1){super(null),this.isData3DTexture=!0,this.image={data:e,width:t,height:i,depth:s},this.magFilter=Rt,this.minFilter=Rt,this.wrapR=Ln,this.generateMipmaps=!1,this.flipY=!1,this.unpackAlignment=1}copy(e){return super.copy(e),this.wrapR=e.wrapR,this}};var ct=class n{static{n.prototype.isMatrix4=!0}constructor(e,t,i,s,r,o,a,l,c,u,d,h,f,g,y,m){this.elements=[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1],e!==void 0&&this.set(e,t,i,s,r,o,a,l,c,u,d,h,f,g,y,m)}set(e,t,i,s,r,o,a,l,c,u,d,h,f,g,y,m){let p=this.elements;return p[0]=e,p[4]=t,p[8]=i,p[12]=s,p[1]=r,p[5]=o,p[9]=a,p[13]=l,p[2]=c,p[6]=u,p[10]=d,p[14]=h,p[3]=f,p[7]=g,p[11]=y,p[15]=m,this}identity(){return this.set(1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1),this}clone(){return new n().fromArray(this.elements)}copy(e){let t=this.elements,i=e.elements;return t[0]=i[0],t[1]=i[1],t[2]=i[2],t[3]=i[3],t[4]=i[4],t[5]=i[5],t[6]=i[6],t[7]=i[7],t[8]=i[8],t[9]=i[9],t[10]=i[10],t[11]=i[11],t[12]=i[12],t[13]=i[13],t[14]=i[14],t[15]=i[15],this}copyPosition(e){let t=this.elements,i=e.elements;return t[12]=i[12],t[13]=i[13],t[14]=i[14],this}setFromMatrix3(e){let t=e.elements;return this.set(t[0],t[3],t[6],0,t[1],t[4],t[7],0,t[2],t[5],t[8],0,0,0,0,1),this}extractBasis(e,t,i){return this.determinantAffine()===0?(e.set(1,0,0),t.set(0,1,0),i.set(0,0,1),this):(e.setFromMatrixColumn(this,0),t.setFromMatrixColumn(this,1),i.setFromMatrixColumn(this,2),this)}makeBasis(e,t,i){return this.set(e.x,t.x,i.x,0,e.y,t.y,i.y,0,e.z,t.z,i.z,0,0,0,0,1),this}extractRotation(e){if(e.determinantAffine()===0)return this.identity();let t=this.elements,i=e.elements,s=1/Xs.setFromMatrixColumn(e,0).length(),r=1/Xs.setFromMatrixColumn(e,1).length(),o=1/Xs.setFromMatrixColumn(e,2).length();return t[0]=i[0]*s,t[1]=i[1]*s,t[2]=i[2]*s,t[3]=0,t[4]=i[4]*r,t[5]=i[5]*r,t[6]=i[6]*r,t[7]=0,t[8]=i[8]*o,t[9]=i[9]*o,t[10]=i[10]*o,t[11]=0,t[12]=0,t[13]=0,t[14]=0,t[15]=1,this}makeRotationFromEuler(e){let t=this.elements,i=e.x,s=e.y,r=e.z,o=Math.cos(i),a=Math.sin(i),l=Math.cos(s),c=Math.sin(s),u=Math.cos(r),d=Math.sin(r);if(e.order==="XYZ"){let h=o*u,f=o*d,g=a*u,y=a*d;t[0]=l*u,t[4]=-l*d,t[8]=c,t[1]=f+g*c,t[5]=h-y*c,t[9]=-a*l,t[2]=y-h*c,t[6]=g+f*c,t[10]=o*l}else if(e.order==="YXZ"){let h=l*u,f=l*d,g=c*u,y=c*d;t[0]=h+y*a,t[4]=g*a-f,t[8]=o*c,t[1]=o*d,t[5]=o*u,t[9]=-a,t[2]=f*a-g,t[6]=y+h*a,t[10]=o*l}else if(e.order==="ZXY"){let h=l*u,f=l*d,g=c*u,y=c*d;t[0]=h-y*a,t[4]=-o*d,t[8]=g+f*a,t[1]=f+g*a,t[5]=o*u,t[9]=y-h*a,t[2]=-o*c,t[6]=a,t[10]=o*l}else if(e.order==="ZYX"){let h=o*u,f=o*d,g=a*u,y=a*d;t[0]=l*u,t[4]=g*c-f,t[8]=h*c+y,t[1]=l*d,t[5]=y*c+h,t[9]=f*c-g,t[2]=-c,t[6]=a*l,t[10]=o*l}else if(e.order==="YZX"){let h=o*l,f=o*c,g=a*l,y=a*c;t[0]=l*u,t[4]=y-h*d,t[8]=g*d+f,t[1]=d,t[5]=o*u,t[9]=-a*u,t[2]=-c*u,t[6]=f*d+g,t[10]=h-y*d}else if(e.order==="XZY"){let h=o*l,f=o*c,g=a*l,y=a*c;t[0]=l*u,t[4]=-d,t[8]=c*u,t[1]=h*d+y,t[5]=o*u,t[9]=f*d-g,t[2]=g*d-f,t[6]=a*u,t[10]=y*d+h}return t[3]=0,t[7]=0,t[11]=0,t[12]=0,t[13]=0,t[14]=0,t[15]=1,this}makeRotationFromQuaternion(e){return this.compose(X0,e,K0)}lookAt(e,t,i){let s=this.elements;return En.subVectors(e,t),En.lengthSq()===0&&(En.z=1),En.normalize(),Gi.crossVectors(i,En),Gi.lengthSq()===0&&(Math.abs(i.z)===1?En.x+=1e-4:En.z+=1e-4,En.normalize(),Gi.crossVectors(i,En)),Gi.normalize(),Qa.crossVectors(En,Gi),s[0]=Gi.x,s[4]=Qa.x,s[8]=En.x,s[1]=Gi.y,s[5]=Qa.y,s[9]=En.y,s[2]=Gi.z,s[6]=Qa.z,s[10]=En.z,this}multiply(e){return this.multiplyMatrices(this,e)}premultiply(e){return this.multiplyMatrices(e,this)}multiplyMatrices(e,t){let i=e.elements,s=t.elements,r=this.elements,o=i[0],a=i[4],l=i[8],c=i[12],u=i[1],d=i[5],h=i[9],f=i[13],g=i[2],y=i[6],m=i[10],p=i[14],E=i[3],w=i[7],S=i[11],v=i[15],A=s[0],R=s[4],b=s[8],M=s[12],B=s[1],D=s[5],k=s[9],H=s[13],F=s[2],O=s[6],q=s[10],X=s[14],re=s[3],K=s[7],ne=s[11],se=s[15];return r[0]=o*A+a*B+l*F+c*re,r[4]=o*R+a*D+l*O+c*K,r[8]=o*b+a*k+l*q+c*ne,r[12]=o*M+a*H+l*X+c*se,r[1]=u*A+d*B+h*F+f*re,r[5]=u*R+d*D+h*O+f*K,r[9]=u*b+d*k+h*q+f*ne,r[13]=u*M+d*H+h*X+f*se,r[2]=g*A+y*B+m*F+p*re,r[6]=g*R+y*D+m*O+p*K,r[10]=g*b+y*k+m*q+p*ne,r[14]=g*M+y*H+m*X+p*se,r[3]=E*A+w*B+S*F+v*re,r[7]=E*R+w*D+S*O+v*K,r[11]=E*b+w*k+S*q+v*ne,r[15]=E*M+w*H+S*X+v*se,this}multiplyScalar(e){let t=this.elements;return t[0]*=e,t[4]*=e,t[8]*=e,t[12]*=e,t[1]*=e,t[5]*=e,t[9]*=e,t[13]*=e,t[2]*=e,t[6]*=e,t[10]*=e,t[14]*=e,t[3]*=e,t[7]*=e,t[11]*=e,t[15]*=e,this}determinant(){let e=this.elements,t=e[0],i=e[4],s=e[8],r=e[12],o=e[1],a=e[5],l=e[9],c=e[13],u=e[2],d=e[6],h=e[10],f=e[14],g=e[3],y=e[7],m=e[11],p=e[15],E=l*f-c*h,w=a*f-c*d,S=a*h-l*d,v=o*f-c*u,A=o*h-l*u,R=o*d-a*u;return t*(y*E-m*w+p*S)-i*(g*E-m*v+p*A)+s*(g*w-y*v+p*R)-r*(g*S-y*A+m*R)}determinantAffine(){let e=this.elements,t=e[0],i=e[4],s=e[8],r=e[1],o=e[5],a=e[9],l=e[2],c=e[6],u=e[10];return t*(o*u-a*c)-i*(r*u-a*l)+s*(r*c-o*l)}transpose(){let e=this.elements,t;return t=e[1],e[1]=e[4],e[4]=t,t=e[2],e[2]=e[8],e[8]=t,t=e[6],e[6]=e[9],e[9]=t,t=e[3],e[3]=e[12],e[12]=t,t=e[7],e[7]=e[13],e[13]=t,t=e[11],e[11]=e[14],e[14]=t,this}setPosition(e,t,i){let s=this.elements;return e.isVector3?(s[12]=e.x,s[13]=e.y,s[14]=e.z):(s[12]=e,s[13]=t,s[14]=i),this}invert(){let e=this.elements,t=e[0],i=e[1],s=e[2],r=e[3],o=e[4],a=e[5],l=e[6],c=e[7],u=e[8],d=e[9],h=e[10],f=e[11],g=e[12],y=e[13],m=e[14],p=e[15],E=t*a-i*o,w=t*l-s*o,S=t*c-r*o,v=i*l-s*a,A=i*c-r*a,R=s*c-r*l,b=u*y-d*g,M=u*m-h*g,B=u*p-f*g,D=d*m-h*y,k=d*p-f*y,H=h*p-f*m,F=E*H-w*k+S*D+v*B-A*M+R*b;if(F===0)return this.set(0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0);let O=1/F;return e[0]=(a*H-l*k+c*D)*O,e[1]=(s*k-i*H-r*D)*O,e[2]=(y*R-m*A+p*v)*O,e[3]=(h*A-d*R-f*v)*O,e[4]=(l*B-o*H-c*M)*O,e[5]=(t*H-s*B+r*M)*O,e[6]=(m*S-g*R-p*w)*O,e[7]=(u*R-h*S+f*w)*O,e[8]=(o*k-a*B+c*b)*O,e[9]=(i*B-t*k-r*b)*O,e[10]=(g*A-y*S+p*E)*O,e[11]=(d*S-u*A-f*E)*O,e[12]=(a*M-o*D-l*b)*O,e[13]=(t*D-i*M+s*b)*O,e[14]=(y*w-g*v-m*E)*O,e[15]=(u*v-d*w+h*E)*O,this}scale(e){let t=this.elements,i=e.x,s=e.y,r=e.z;return t[0]*=i,t[4]*=s,t[8]*=r,t[1]*=i,t[5]*=s,t[9]*=r,t[2]*=i,t[6]*=s,t[10]*=r,t[3]*=i,t[7]*=s,t[11]*=r,this}getMaxScaleOnAxis(){let e=this.elements,t=e[0]*e[0]+e[1]*e[1]+e[2]*e[2],i=e[4]*e[4]+e[5]*e[5]+e[6]*e[6],s=e[8]*e[8]+e[9]*e[9]+e[10]*e[10];return Math.sqrt(Math.max(t,i,s))}makeTranslation(e,t,i){return e.isVector3?this.set(1,0,0,e.x,0,1,0,e.y,0,0,1,e.z,0,0,0,1):this.set(1,0,0,e,0,1,0,t,0,0,1,i,0,0,0,1),this}makeRotationX(e){let t=Math.cos(e),i=Math.sin(e);return this.set(1,0,0,0,0,t,-i,0,0,i,t,0,0,0,0,1),this}makeRotationY(e){let t=Math.cos(e),i=Math.sin(e);return this.set(t,0,i,0,0,1,0,0,-i,0,t,0,0,0,0,1),this}makeRotationZ(e){let t=Math.cos(e),i=Math.sin(e);return this.set(t,-i,0,0,i,t,0,0,0,0,1,0,0,0,0,1),this}makeRotationAxis(e,t){let i=Math.cos(t),s=Math.sin(t),r=1-i,o=e.x,a=e.y,l=e.z,c=r*o,u=r*a;return this.set(c*o+i,c*a-s*l,c*l+s*a,0,c*a+s*l,u*a+i,u*l-s*o,0,c*l-s*a,u*l+s*o,r*l*l+i,0,0,0,0,1),this}makeScale(e,t,i){return this.set(e,0,0,0,0,t,0,0,0,0,i,0,0,0,0,1),this}makeShear(e,t,i,s,r,o){return this.set(1,i,r,0,e,1,o,0,t,s,1,0,0,0,0,1),this}compose(e,t,i){let s=this.elements,r=t._x,o=t._y,a=t._z,l=t._w,c=r+r,u=o+o,d=a+a,h=r*c,f=r*u,g=r*d,y=o*u,m=o*d,p=a*d,E=l*c,w=l*u,S=l*d,v=i.x,A=i.y,R=i.z;return s[0]=(1-(y+p))*v,s[1]=(f+S)*v,s[2]=(g-w)*v,s[3]=0,s[4]=(f-S)*A,s[5]=(1-(h+p))*A,s[6]=(m+E)*A,s[7]=0,s[8]=(g+w)*R,s[9]=(m-E)*R,s[10]=(1-(h+y))*R,s[11]=0,s[12]=e.x,s[13]=e.y,s[14]=e.z,s[15]=1,this}decompose(e,t,i){let s=this.elements;e.x=s[12],e.y=s[13],e.z=s[14];let r=this.determinantAffine();if(r===0)return i.set(1,1,1),t.identity(),this;let o=Xs.set(s[0],s[1],s[2]).length(),a=Xs.set(s[4],s[5],s[6]).length(),l=Xs.set(s[8],s[9],s[10]).length();r<0&&(o=-o),kn.copy(this);let c=1/o,u=1/a,d=1/l;return kn.elements[0]*=c,kn.elements[1]*=c,kn.elements[2]*=c,kn.elements[4]*=u,kn.elements[5]*=u,kn.elements[6]*=u,kn.elements[8]*=d,kn.elements[9]*=d,kn.elements[10]*=d,t.setFromRotationMatrix(kn),i.x=o,i.y=a,i.z=l,this}makePerspective(e,t,i,s,r,o,a=hn,l=!1){let c=this.elements,u=2*r/(t-e),d=2*r/(i-s),h=(t+e)/(t-e),f=(i+s)/(i-s),g,y;if(l)g=r/(o-r),y=o*r/(o-r);else if(a===hn)g=-(o+r)/(o-r),y=-2*o*r/(o-r);else if(a===li)g=-o/(o-r),y=-o*r/(o-r);else throw new Error("THREE.Matrix4.makePerspective(): Invalid coordinate system: "+a);return c[0]=u,c[4]=0,c[8]=h,c[12]=0,c[1]=0,c[5]=d,c[9]=f,c[13]=0,c[2]=0,c[6]=0,c[10]=g,c[14]=y,c[3]=0,c[7]=0,c[11]=-1,c[15]=0,this}makeOrthographic(e,t,i,s,r,o,a=hn,l=!1){let c=this.elements,u=2/(t-e),d=2/(i-s),h=-(t+e)/(t-e),f=-(i+s)/(i-s),g,y;if(l)g=1/(o-r),y=o/(o-r);else if(a===hn)g=-2/(o-r),y=-(o+r)/(o-r);else if(a===li)g=-1/(o-r),y=-r/(o-r);else throw new Error("THREE.Matrix4.makeOrthographic(): Invalid coordinate system: "+a);return c[0]=u,c[4]=0,c[8]=0,c[12]=h,c[1]=0,c[5]=d,c[9]=0,c[13]=f,c[2]=0,c[6]=0,c[10]=g,c[14]=y,c[3]=0,c[7]=0,c[11]=0,c[15]=1,this}equals(e){let t=this.elements,i=e.elements;for(let s=0;s<16;s++)if(t[s]!==i[s])return!1;return!0}fromArray(e,t=0){for(let i=0;i<16;i++)this.elements[i]=e[i+t];return this}toArray(e=[],t=0){let i=this.elements;return e[t]=i[0],e[t+1]=i[1],e[t+2]=i[2],e[t+3]=i[3],e[t+4]=i[4],e[t+5]=i[5],e[t+6]=i[6],e[t+7]=i[7],e[t+8]=i[8],e[t+9]=i[9],e[t+10]=i[10],e[t+11]=i[11],e[t+12]=i[12],e[t+13]=i[13],e[t+14]=i[14],e[t+15]=i[15],e}},Xs=new L,kn=new ct,X0=new L(0,0,0),K0=new L(1,1,1),Gi=new L,Qa=new L,En=new L,wp=new ct,Cp=new Xn,Ii=class n{constructor(e=0,t=0,i=0,s=n.DEFAULT_ORDER){this.isEuler=!0,this._x=e,this._y=t,this._z=i,this._order=s}get x(){return this._x}set x(e){this._x=e,this._onChangeCallback()}get y(){return this._y}set y(e){this._y=e,this._onChangeCallback()}get z(){return this._z}set z(e){this._z=e,this._onChangeCallback()}get order(){return this._order}set order(e){this._order=e,this._onChangeCallback()}set(e,t,i,s=this._order){return this._x=e,this._y=t,this._z=i,this._order=s,this._onChangeCallback(),this}clone(){return new this.constructor(this._x,this._y,this._z,this._order)}copy(e){return this._x=e._x,this._y=e._y,this._z=e._z,this._order=e._order,this._onChangeCallback(),this}setFromRotationMatrix(e,t=this._order,i=!0){let s=e.elements,r=s[0],o=s[4],a=s[8],l=s[1],c=s[5],u=s[9],d=s[2],h=s[6],f=s[10];switch(t){case"XYZ":this._y=Math.asin(qe(a,-1,1)),Math.abs(a)<.9999999?(this._x=Math.atan2(-u,f),this._z=Math.atan2(-o,r)):(this._x=Math.atan2(h,c),this._z=0);break;case"YXZ":this._x=Math.asin(-qe(u,-1,1)),Math.abs(u)<.9999999?(this._y=Math.atan2(a,f),this._z=Math.atan2(l,c)):(this._y=Math.atan2(-d,r),this._z=0);break;case"ZXY":this._x=Math.asin(qe(h,-1,1)),Math.abs(h)<.9999999?(this._y=Math.atan2(-d,f),this._z=Math.atan2(-o,c)):(this._y=0,this._z=Math.atan2(l,r));break;case"ZYX":this._y=Math.asin(-qe(d,-1,1)),Math.abs(d)<.9999999?(this._x=Math.atan2(h,f),this._z=Math.atan2(l,r)):(this._x=0,this._z=Math.atan2(-o,c));break;case"YZX":this._z=Math.asin(qe(l,-1,1)),Math.abs(l)<.9999999?(this._x=Math.atan2(-u,c),this._y=Math.atan2(-d,r)):(this._x=0,this._y=Math.atan2(a,f));break;case"XZY":this._z=Math.asin(-qe(o,-1,1)),Math.abs(o)<.9999999?(this._x=Math.atan2(h,c),this._y=Math.atan2(a,r)):(this._x=Math.atan2(-u,f),this._y=0);break;default:pe("Euler: .setFromRotationMatrix() encountered an unknown order: "+t)}return this._order=t,i===!0&&this._onChangeCallback(),this}setFromQuaternion(e,t,i){return wp.makeRotationFromQuaternion(e),this.setFromRotationMatrix(wp,t,i)}setFromVector3(e,t=this._order){return this.set(e.x,e.y,e.z,t)}reorder(e){return Cp.setFromEuler(this),this.setFromQuaternion(Cp,e)}equals(e){return e._x===this._x&&e._y===this._y&&e._z===this._z&&e._order===this._order}fromArray(e){return this._x=e[0],this._y=e[1],this._z=e[2],e[3]!==void 0&&(this._order=e[3]),this._onChangeCallback(),this}toArray(e=[],t=0){return e[t]=this._x,e[t+1]=this._y,e[t+2]=this._z,e[t+3]=this._order,e}_onChange(e){return this._onChangeCallback=e,this}_onChangeCallback(){}*[Symbol.iterator](){yield this._x,yield this._y,yield this._z,yield this._order}};Ii.DEFAULT_ORDER="XYZ";var pr=class{constructor(){this.mask=1}set(e){this.mask=(1<<e|0)>>>0}enable(e){this.mask|=1<<e|0}enableAll(){this.mask=-1}toggle(e){this.mask^=1<<e|0}disable(e){this.mask&=~(1<<e|0)}disableAll(){this.mask=0}test(e){return(this.mask&e.mask)!==0}isEnabled(e){return(this.mask&(1<<e|0))!==0}},$0=0,Np=new L,Ks=new Xn,vi=new ct,ja=new L,Qr=new L,J0=new L,q0=new Xn,Ip=new L(1,0,0),Bp=new L(0,1,0),Fp=new L(0,0,1),Lp={type:"added"},Q0={type:"removed"},$s={type:"childadded",child:null},Cu={type:"childremoved",child:null},Jt=class n extends wn{constructor(){super(),this.isObject3D=!0,Object.defineProperty(this,"id",{value:$0++}),this.uuid=Ci(),this.name="",this.type="Object3D",this.parent=null,this.children=[],this.up=n.DEFAULT_UP.clone();let e=new L,t=new Ii,i=new Xn,s=new L(1,1,1);function r(){i.setFromEuler(t,!1)}function o(){t.setFromQuaternion(i,void 0,!1)}t._onChange(r),i._onChange(o),Object.defineProperties(this,{position:{configurable:!0,enumerable:!0,value:e},rotation:{configurable:!0,enumerable:!0,value:t},quaternion:{configurable:!0,enumerable:!0,value:i},scale:{configurable:!0,enumerable:!0,value:s},modelViewMatrix:{value:new ct},normalMatrix:{value:new Ge}}),this.matrix=new ct,this.matrixWorld=new ct,this.matrixAutoUpdate=n.DEFAULT_MATRIX_AUTO_UPDATE,this.matrixWorldAutoUpdate=n.DEFAULT_MATRIX_WORLD_AUTO_UPDATE,this.matrixWorldNeedsUpdate=!1,this.layers=new pr,this.visible=!0,this.castShadow=!1,this.receiveShadow=!1,this.frustumCulled=!0,this.renderOrder=0,this.animations=[],this.customDepthMaterial=void 0,this.customDistanceMaterial=void 0,this.static=!1,this.userData={},this.pivot=null}onBeforeShadow(){}onAfterShadow(){}onBeforeRender(){}onAfterRender(){}applyMatrix4(e){this.matrixAutoUpdate&&this.updateMatrix(),this.matrix.premultiply(e),this.matrix.decompose(this.position,this.quaternion,this.scale)}applyQuaternion(e){return this.quaternion.premultiply(e),this}setRotationFromAxisAngle(e,t){this.quaternion.setFromAxisAngle(e,t)}setRotationFromEuler(e){this.quaternion.setFromEuler(e,!0)}setRotationFromMatrix(e){this.quaternion.setFromRotationMatrix(e)}setRotationFromQuaternion(e){this.quaternion.copy(e)}rotateOnAxis(e,t){return Ks.setFromAxisAngle(e,t),this.quaternion.multiply(Ks),this}rotateOnWorldAxis(e,t){return Ks.setFromAxisAngle(e,t),this.quaternion.premultiply(Ks),this}rotateX(e){return this.rotateOnAxis(Ip,e)}rotateY(e){return this.rotateOnAxis(Bp,e)}rotateZ(e){return this.rotateOnAxis(Fp,e)}translateOnAxis(e,t){return Np.copy(e).applyQuaternion(this.quaternion),this.position.add(Np.multiplyScalar(t)),this}translateX(e){return this.translateOnAxis(Ip,e)}translateY(e){return this.translateOnAxis(Bp,e)}translateZ(e){return this.translateOnAxis(Fp,e)}localToWorld(e){return this.updateWorldMatrix(!0,!1),e.applyMatrix4(this.matrixWorld)}worldToLocal(e){return this.updateWorldMatrix(!0,!1),e.applyMatrix4(vi.copy(this.matrixWorld).invert())}lookAt(e,t,i){e.isVector3?ja.copy(e):ja.set(e,t,i);let s=this.parent;this.updateWorldMatrix(!0,!1),Qr.setFromMatrixPosition(this.matrixWorld),this.isCamera||this.isLight?vi.lookAt(Qr,ja,this.up):vi.lookAt(ja,Qr,this.up),this.quaternion.setFromRotationMatrix(vi),s&&(vi.extractRotation(s.matrixWorld),Ks.setFromRotationMatrix(vi),this.quaternion.premultiply(Ks.invert()))}add(e){if(arguments.length>1){for(let t=0;t<arguments.length;t++)this.add(arguments[t]);return this}return e===this?(_e("Object3D.add: object can't be added as a child of itself.",e),this):(e&&e.isObject3D?(e.removeFromParent(),e.parent=this,this.children.push(e),e.dispatchEvent(Lp),$s.child=e,this.dispatchEvent($s),$s.child=null):_e("Object3D.add: object not an instance of THREE.Object3D.",e),this)}remove(e){if(arguments.length>1){for(let i=0;i<arguments.length;i++)this.remove(arguments[i]);return this}let t=this.children.indexOf(e);return t!==-1&&(e.parent=null,this.children.splice(t,1),e.dispatchEvent(Q0),Cu.child=e,this.dispatchEvent(Cu),Cu.child=null),this}removeFromParent(){let e=this.parent;return e!==null&&e.remove(this),this}clear(){return this.remove(...this.children)}attach(e){return this.updateWorldMatrix(!0,!1),vi.copy(this.matrixWorld).invert(),e.parent!==null&&(e.parent.updateWorldMatrix(!0,!1),vi.multiply(e.parent.matrixWorld)),e.applyMatrix4(vi),e.removeFromParent(),e.parent=this,this.children.push(e),e.updateWorldMatrix(!1,!0),e.dispatchEvent(Lp),$s.child=e,this.dispatchEvent($s),$s.child=null,this}getObjectById(e){return this.getObjectByProperty("id",e)}getObjectByName(e){return this.getObjectByProperty("name",e)}getObjectByProperty(e,t){if(this[e]===t)return this;for(let i=0,s=this.children.length;i<s;i++){let o=this.children[i].getObjectByProperty(e,t);if(o!==void 0)return o}}getObjectsByProperty(e,t,i=[]){this[e]===t&&i.push(this);let s=this.children;for(let r=0,o=s.length;r<o;r++)s[r].getObjectsByProperty(e,t,i);return i}getWorldPosition(e){return this.updateWorldMatrix(!0,!1),e.setFromMatrixPosition(this.matrixWorld)}getWorldQuaternion(e){return this.updateWorldMatrix(!0,!1),this.matrixWorld.decompose(Qr,e,J0),e}getWorldScale(e){return this.updateWorldMatrix(!0,!1),this.matrixWorld.decompose(Qr,q0,e),e}getWorldDirection(e){this.updateWorldMatrix(!0,!1);let t=this.matrixWorld.elements;return e.set(t[8],t[9],t[10]).normalize()}raycast(){}intersectsFrustum(){}traverse(e){e(this);let t=this.children;for(let i=0,s=t.length;i<s;i++)t[i].traverse(e)}traverseVisible(e){if(this.visible===!1)return;e(this);let t=this.children;for(let i=0,s=t.length;i<s;i++)t[i].traverseVisible(e)}traverseAncestors(e){let t=this.parent;t!==null&&(e(t),t.traverseAncestors(e))}updateMatrix(){this.matrix.compose(this.position,this.quaternion,this.scale);let e=this.pivot;if(e!==null){let t=e.x,i=e.y,s=e.z,r=this.matrix.elements;r[12]+=t-r[0]*t-r[4]*i-r[8]*s,r[13]+=i-r[1]*t-r[5]*i-r[9]*s,r[14]+=s-r[2]*t-r[6]*i-r[10]*s}this.matrixWorldNeedsUpdate=!0}updateMatrixWorld(e){this.matrixAutoUpdate&&this.updateMatrix(),(this.matrixWorldNeedsUpdate||e)&&(this.matrixWorldAutoUpdate===!0&&(this.parent===null?this.matrixWorld.copy(this.matrix):this.matrixWorld.multiplyMatrices(this.parent.matrixWorld,this.matrix)),this.matrixWorldNeedsUpdate=!1,e=!0);let t=this.children;for(let i=0,s=t.length;i<s;i++)t[i].updateMatrixWorld(e)}updateWorldMatrix(e,t,i=!1){let s=this.parent;if(e===!0&&s!==null&&s.updateWorldMatrix(!0,!1),this.matrixAutoUpdate&&this.updateMatrix(),(this.matrixWorldNeedsUpdate||i)&&(this.matrixWorldAutoUpdate===!0&&(this.parent===null?this.matrixWorld.copy(this.matrix):this.matrixWorld.multiplyMatrices(this.parent.matrixWorld,this.matrix)),this.matrixWorldNeedsUpdate=!1,i=!0),t===!0){let r=this.children;for(let o=0,a=r.length;o<a;o++)r[o].updateWorldMatrix(!1,!0,i)}}toJSON(e){let t=e===void 0||typeof e=="string",i={};t&&(e={geometries:{},materials:{},textures:{},images:{},shapes:{},skeletons:{},animations:{},nodes:{}},i.metadata={version:4.7,type:"Object",generator:"Object3D.toJSON"});let s={};s.uuid=this.uuid,s.type=this.type,s.name=this.name,s.castShadow=this.castShadow,s.receiveShadow=this.receiveShadow,s.visible=this.visible,s.frustumCulled=this.frustumCulled,s.renderOrder=this.renderOrder,s.static=this.static,s.matrixAutoUpdate=this.matrixAutoUpdate,Object.keys(this.userData).length>0&&(s.userData=this.userData),s.layers=this.layers.mask,s.matrix=this.matrix.toArray(),s.up=this.up.toArray(),this.pivot!==null&&(s.pivot=this.pivot.toArray()),this.morphTargetDictionary!==void 0&&(s.morphTargetDictionary=Object.assign({},this.morphTargetDictionary)),this.morphTargetInfluences!==void 0&&(s.morphTargetInfluences=this.morphTargetInfluences.slice()),this.isInstancedMesh&&(s.type="InstancedMesh",s.count=this.count,s.instanceMatrix=this.instanceMatrix.toJSON(),this.instanceColor!==null&&(s.instanceColor=this.instanceColor.toJSON())),this.isBatchedMesh&&(s.type="BatchedMesh",s.perObjectFrustumCulled=this.perObjectFrustumCulled,s.sortObjects=this.sortObjects,s.drawRanges=this._drawRanges,s.reservedRanges=this._reservedRanges,s.geometryInfo=this._geometryInfo.map(a=>({...a,boundingBox:a.boundingBox?a.boundingBox.toJSON():void 0,boundingSphere:a.boundingSphere?a.boundingSphere.toJSON():void 0})),s.instanceInfo=this._instanceInfo.map(a=>({...a})),s.availableInstanceIds=this._availableInstanceIds.slice(),s.availableGeometryIds=this._availableGeometryIds.slice(),s.nextIndexStart=this._nextIndexStart,s.nextVertexStart=this._nextVertexStart,s.geometryCount=this._geometryCount,s.maxInstanceCount=this._maxInstanceCount,s.maxVertexCount=this._maxVertexCount,s.maxIndexCount=this._maxIndexCount,s.geometryInitialized=this._geometryInitialized,s.matricesTexture=this._matricesTexture.toJSON(e),s.indirectTexture=this._indirectTexture.toJSON(e),this._colorsTexture!==null&&(s.colorsTexture=this._colorsTexture.toJSON(e)),this.boundingSphere!==null&&(s.boundingSphere=this.boundingSphere.toJSON()),this.boundingBox!==null&&(s.boundingBox=this.boundingBox.toJSON()));function r(a,l){return a[l.uuid]===void 0&&(a[l.uuid]=l.toJSON(e)),l.uuid}if(this.isScene)this.background&&(this.background.isColor?s.background=this.background.toJSON():this.background.isTexture&&(s.background=this.background.toJSON(e).uuid)),this.environment&&this.environment.isTexture&&this.environment.isRenderTargetTexture!==!0&&(s.environment=this.environment.toJSON(e).uuid);else if(this.isMesh||this.isLine||this.isPoints){s.geometry=r(e.geometries,this.geometry);let a=this.geometry.parameters;if(a!==void 0&&a.shapes!==void 0){let l=a.shapes;if(Array.isArray(l))for(let c=0,u=l.length;c<u;c++){let d=l[c];r(e.shapes,d)}else r(e.shapes,l)}}if(this.isSkinnedMesh&&(s.bindMode=this.bindMode,s.bindMatrix=this.bindMatrix.toArray(),this.skeleton!==void 0&&(r(e.skeletons,this.skeleton),s.skeleton=this.skeleton.uuid)),this.material!==void 0)if(Array.isArray(this.material)){let a=[];for(let l=0,c=this.material.length;l<c;l++)a.push(r(e.materials,this.material[l]));s.material=a}else s.material=r(e.materials,this.material);if(this.children.length>0){s.children=[];for(let a=0;a<this.children.length;a++)s.children.push(this.children[a].toJSON(e).object)}if(this.animations.length>0){s.animations=[];for(let a=0;a<this.animations.length;a++){let l=this.animations[a];s.animations.push(r(e.animations,l))}}if(t){let a=o(e.geometries),l=o(e.materials),c=o(e.textures),u=o(e.images),d=o(e.shapes),h=o(e.skeletons),f=o(e.animations),g=o(e.nodes);a.length>0&&(i.geometries=a),l.length>0&&(i.materials=l),c.length>0&&(i.textures=c),u.length>0&&(i.images=u),d.length>0&&(i.shapes=d),h.length>0&&(i.skeletons=h),f.length>0&&(i.animations=f),g.length>0&&(i.nodes=g)}return i.object=s,i;function o(a){let l=[];for(let c in a){let u=a[c];delete u.metadata,l.push(u)}return l}}clone(e){return new this.constructor().copy(this,e)}copy(e,t=!0){if(this.name=e.name,this.up.copy(e.up),this.position.copy(e.position),this.rotation.order=e.rotation.order,this.quaternion.copy(e.quaternion),this.scale.copy(e.scale),this.pivot=e.pivot!==null?e.pivot.clone():null,this.matrix.copy(e.matrix),this.matrixWorld.copy(e.matrixWorld),this.matrixAutoUpdate=e.matrixAutoUpdate,this.matrixWorldAutoUpdate=e.matrixWorldAutoUpdate,this.matrixWorldNeedsUpdate=e.matrixWorldNeedsUpdate,this.layers.mask=e.layers.mask,this.visible=e.visible,this.castShadow=e.castShadow,this.receiveShadow=e.receiveShadow,this.frustumCulled=e.frustumCulled,this.renderOrder=e.renderOrder,this.static=e.static,this.animations=e.animations.slice(),this.userData=JSON.parse(JSON.stringify(e.userData)),t===!0)for(let i=0;i<e.children.length;i++){let s=e.children[i];this.add(s.clone())}return this}dispose(){this.dispatchEvent({type:"dispose"})}};Jt.DEFAULT_UP=new L(0,1,0);Jt.DEFAULT_MATRIX_AUTO_UPDATE=!0;Jt.DEFAULT_MATRIX_WORLD_AUTO_UPDATE=!0;var Yn=class extends Jt{constructor(){super(),this.isGroup=!0,this.type="Group"}},j0={type:"move"},fs=class{constructor(){this._targetRay=null,this._grip=null,this._hand=null}getHandSpace(){return this._hand===null&&(this._hand=new Yn,this._hand.matrixAutoUpdate=!1,this._hand.visible=!1,this._hand.joints={},this._hand.inputState={pinching:!1}),this._hand}getTargetRaySpace(){return this._targetRay===null&&(this._targetRay=new Yn,this._targetRay.matrixAutoUpdate=!1,this._targetRay.visible=!1,this._targetRay.hasLinearVelocity=!1,this._targetRay.linearVelocity=new L,this._targetRay.hasAngularVelocity=!1,this._targetRay.angularVelocity=new L),this._targetRay}getGripSpace(){return this._grip===null&&(this._grip=new Yn,this._grip.matrixAutoUpdate=!1,this._grip.visible=!1,this._grip.hasLinearVelocity=!1,this._grip.linearVelocity=new L,this._grip.hasAngularVelocity=!1,this._grip.angularVelocity=new L,this._grip.eventsEnabled=!1),this._grip}dispatchEvent(e){return this._targetRay!==null&&this._targetRay.dispatchEvent(e),this._grip!==null&&this._grip.dispatchEvent(e),this._hand!==null&&this._hand.dispatchEvent(e),this}connect(e){if(e&&e.hand){let t=this._hand;if(t)for(let i of e.hand.values())this._getHandJoint(t,i)}return this.dispatchEvent({type:"connected",data:e}),this}disconnect(e){return this.dispatchEvent({type:"disconnected",data:e}),this._targetRay!==null&&(this._targetRay.visible=!1),this._grip!==null&&(this._grip.visible=!1),this._hand!==null&&(this._hand.visible=!1),this}update(e,t,i){let s=null,r=null,o=null,a=this._targetRay,l=this._grip,c=this._hand;if(e&&t.session.visibilityState!=="visible-blurred"){if(c&&e.hand){o=!0;for(let y of e.hand.values()){let m=t.getJointPose(y,i),p=this._getHandJoint(c,y);m!==null&&(p.matrix.fromArray(m.transform.matrix),p.matrix.decompose(p.position,p.rotation,p.scale),p.matrixWorldNeedsUpdate=!0,p.jointRadius=m.radius),p.visible=m!==null}let u=c.joints["index-finger-tip"],d=c.joints["thumb-tip"],h=u.position.distanceTo(d.position),f=.02,g=.005;c.inputState.pinching&&h>f+g?(c.inputState.pinching=!1,this.dispatchEvent({type:"pinchend",handedness:e.handedness,target:this})):!c.inputState.pinching&&h<=f-g&&(c.inputState.pinching=!0,this.dispatchEvent({type:"pinchstart",handedness:e.handedness,target:this}))}else l!==null&&e.gripSpace&&(r=t.getPose(e.gripSpace,i),r!==null&&(l.matrix.fromArray(r.transform.matrix),l.matrix.decompose(l.position,l.rotation,l.scale),l.matrixWorldNeedsUpdate=!0,r.linearVelocity?(l.hasLinearVelocity=!0,l.linearVelocity.copy(r.linearVelocity)):l.hasLinearVelocity=!1,r.angularVelocity?(l.hasAngularVelocity=!0,l.angularVelocity.copy(r.angularVelocity)):l.hasAngularVelocity=!1,l.eventsEnabled&&l.dispatchEvent({type:"gripUpdated",data:e,target:this})));a!==null&&(s=t.getPose(e.targetRaySpace,i),s===null&&r!==null&&(s=r),s!==null&&(a.matrix.fromArray(s.transform.matrix),a.matrix.decompose(a.position,a.rotation,a.scale),a.matrixWorldNeedsUpdate=!0,s.linearVelocity?(a.hasLinearVelocity=!0,a.linearVelocity.copy(s.linearVelocity)):a.hasLinearVelocity=!1,s.angularVelocity?(a.hasAngularVelocity=!0,a.angularVelocity.copy(s.angularVelocity)):a.hasAngularVelocity=!1,this.dispatchEvent(j0)))}return a!==null&&(a.visible=s!==null),l!==null&&(l.visible=r!==null),c!==null&&(c.visible=o!==null),this}_getHandJoint(e,t){if(e.joints[t.jointName]===void 0){let i=new Yn;i.matrixAutoUpdate=!1,i.visible=!1,e.joints[t.jointName]=i,e.add(i)}return e.joints[t.jointName]}},tm={aliceblue:15792383,antiquewhite:16444375,aqua:65535,aquamarine:8388564,azure:15794175,beige:16119260,bisque:16770244,black:0,blanchedalmond:16772045,blue:255,blueviolet:9055202,brown:10824234,burlywood:14596231,cadetblue:6266528,chartreuse:8388352,chocolate:13789470,coral:16744272,cornflowerblue:6591981,cornsilk:16775388,crimson:14423100,cyan:65535,darkblue:139,darkcyan:35723,darkgoldenrod:12092939,darkgray:11119017,darkgreen:25600,darkgrey:11119017,darkkhaki:12433259,darkmagenta:9109643,darkolivegreen:5597999,darkorange:16747520,darkorchid:10040012,darkred:9109504,darksalmon:15308410,darkseagreen:9419919,darkslateblue:4734347,darkslategray:3100495,darkslategrey:3100495,darkturquoise:52945,darkviolet:9699539,deeppink:16716947,deepskyblue:49151,dimgray:6908265,dimgrey:6908265,dodgerblue:2003199,firebrick:11674146,floralwhite:16775920,forestgreen:2263842,fuchsia:16711935,gainsboro:14474460,ghostwhite:16316671,gold:16766720,goldenrod:14329120,gray:8421504,green:32768,greenyellow:11403055,grey:8421504,honeydew:15794160,hotpink:16738740,indianred:13458524,indigo:4915330,ivory:16777200,khaki:15787660,lavender:15132410,lavenderblush:16773365,lawngreen:8190976,lemonchiffon:16775885,lightblue:11393254,lightcoral:15761536,lightcyan:14745599,lightgoldenrodyellow:16448210,lightgray:13882323,lightgreen:9498256,lightgrey:13882323,lightpink:16758465,lightsalmon:16752762,lightseagreen:2142890,lightskyblue:8900346,lightslategray:7833753,lightslategrey:7833753,lightsteelblue:11584734,lightyellow:16777184,lime:65280,limegreen:3329330,linen:16445670,magenta:16711935,maroon:8388608,mediumaquamarine:6737322,mediumblue:205,mediumorchid:12211667,mediumpurple:9662683,mediumseagreen:3978097,mediumslateblue:8087790,mediumspringgreen:64154,mediumturquoise:4772300,mediumvioletred:13047173,midnightblue:1644912,mintcream:16121850,mistyrose:16770273,moccasin:16770229,navajowhite:16768685,navy:128,oldlace:16643558,olive:8421376,olivedrab:7048739,orange:16753920,orangered:16729344,orchid:14315734,palegoldenrod:15657130,palegreen:10025880,paleturquoise:11529966,palevioletred:14381203,papayawhip:16773077,peachpuff:16767673,peru:13468991,pink:16761035,plum:14524637,powderblue:11591910,purple:8388736,rebeccapurple:6697881,red:16711680,rosybrown:12357519,royalblue:4286945,saddlebrown:9127187,salmon:16416882,sandybrown:16032864,seagreen:3050327,seashell:16774638,sienna:10506797,silver:12632256,skyblue:8900331,slateblue:6970061,slategray:7372944,slategrey:7372944,snow:16775930,springgreen:65407,steelblue:4620980,tan:13808780,teal:32896,thistle:14204888,tomato:16737095,turquoise:4251856,violet:15631086,wheat:16113331,white:16777215,whitesmoke:16119285,yellow:16776960,yellowgreen:10145074},Vi={h:0,s:0,l:0},el={h:0,s:0,l:0};function Nu(n,e,t){return t<0&&(t+=1),t>1&&(t-=1),t<1/6?n+(e-n)*6*t:t<1/2?e:t<2/3?n+(e-n)*6*(2/3-t):n}var He=class{constructor(e,t,i){return this.isColor=!0,this.r=1,this.g=1,this.b=1,this.set(e,t,i)}set(e,t,i){if(t===void 0&&i===void 0){let s=e;s&&s.isColor?this.copy(s):typeof s=="number"?this.setHex(s):typeof s=="string"&&this.setStyle(s)}else this.setRGB(e,t,i);return this}setScalar(e){return this.r=e,this.g=e,this.b=e,this}setHex(e,t=yn){return e=Math.floor(e),this.r=(e>>16&255)/255,this.g=(e>>8&255)/255,this.b=(e&255)/255,Ve.colorSpaceToWorking(this,t),this}setRGB(e,t,i,s=Ve.workingColorSpace){return this.r=e,this.g=t,this.b=i,Ve.colorSpaceToWorking(this,s),this}setHSL(e,t,i,s=Ve.workingColorSpace){if(e=Fh(e,1),t=qe(t,0,1),i=qe(i,0,1),t===0)this.r=this.g=this.b=i;else{let r=i<=.5?i*(1+t):i+t-i*t,o=2*i-r;this.r=Nu(o,r,e+1/3),this.g=Nu(o,r,e),this.b=Nu(o,r,e-1/3)}return Ve.colorSpaceToWorking(this,s),this}setStyle(e,t=yn){function i(r){r!==void 0&&parseFloat(r)<1&&pe("Color: Alpha component of "+e+" will be ignored.")}let s;if(s=/^(\w+)\(([^\)]*)\)/.exec(e)){let r,o=s[1],a=s[2];switch(o){case"rgb":case"rgba":if(r=/^\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*(\d*\.?\d+)\s*)?$/.exec(a))return i(r[4]),this.setRGB(Math.min(255,parseInt(r[1],10))/255,Math.min(255,parseInt(r[2],10))/255,Math.min(255,parseInt(r[3],10))/255,t);if(r=/^\s*(\d+)\%\s*,\s*(\d+)\%\s*,\s*(\d+)\%\s*(?:,\s*(\d*\.?\d+)\s*)?$/.exec(a))return i(r[4]),this.setRGB(Math.min(100,parseInt(r[1],10))/100,Math.min(100,parseInt(r[2],10))/100,Math.min(100,parseInt(r[3],10))/100,t);break;case"hsl":case"hsla":if(r=/^\s*(\d*\.?\d+)\s*,\s*(\d*\.?\d+)\%\s*,\s*(\d*\.?\d+)\%\s*(?:,\s*(\d*\.?\d+)\s*)?$/.exec(a))return i(r[4]),this.setHSL(parseFloat(r[1])/360,parseFloat(r[2])/100,parseFloat(r[3])/100,t);break;default:pe("Color: Unknown color model "+e)}}else if(s=/^\#([A-Fa-f\d]+)$/.exec(e)){let r=s[1],o=r.length;if(o===3)return this.setRGB(parseInt(r.charAt(0),16)/15,parseInt(r.charAt(1),16)/15,parseInt(r.charAt(2),16)/15,t);if(o===6)return this.setHex(parseInt(r,16),t);pe("Color: Invalid hex color "+e)}else if(e&&e.length>0)return this.setColorName(e,t);return this}setColorName(e,t=yn){let i=tm[e.toLowerCase()];return i!==void 0?this.setHex(i,t):pe("Color: Unknown color "+e),this}clone(){return new this.constructor(this.r,this.g,this.b)}copy(e){return this.r=e.r,this.g=e.g,this.b=e.b,this}copySRGBToLinear(e){return this.r=Ni(e.r),this.g=Ni(e.g),this.b=Ni(e.b),this}copyLinearToSRGB(e){return this.r=or(e.r),this.g=or(e.g),this.b=or(e.b),this}convertSRGBToLinear(){return this.copySRGBToLinear(this),this}convertLinearToSRGB(){return this.copyLinearToSRGB(this),this}getHex(e=yn){return Ve.workingToColorSpace(tn.copy(this),e),Math.round(qe(tn.r*255,0,255))*65536+Math.round(qe(tn.g*255,0,255))*256+Math.round(qe(tn.b*255,0,255))}getHexString(e=yn){return("000000"+this.getHex(e).toString(16)).slice(-6)}getHSL(e,t=Ve.workingColorSpace){Ve.workingToColorSpace(tn.copy(this),t);let i=tn.r,s=tn.g,r=tn.b,o=Math.max(i,s,r),a=Math.min(i,s,r),l,c,u=(a+o)/2;if(a===o)l=0,c=0;else{let d=o-a;switch(c=u<=.5?d/(o+a):d/(2-o-a),o){case i:l=(s-r)/d+(s<r?6:0);break;case s:l=(r-i)/d+2;break;case r:l=(i-s)/d+4;break}l/=6}return e.h=l,e.s=c,e.l=u,e}getRGB(e,t=Ve.workingColorSpace){return Ve.workingToColorSpace(tn.copy(this),t),e.r=tn.r,e.g=tn.g,e.b=tn.b,e}getStyle(e=yn){Ve.workingToColorSpace(tn.copy(this),e);let t=tn.r,i=tn.g,s=tn.b;return e!==yn?`color(${e} ${t.toFixed(3)} ${i.toFixed(3)} ${s.toFixed(3)})`:`rgb(${Math.round(t*255)},${Math.round(i*255)},${Math.round(s*255)})`}offsetHSL(e,t,i){return this.getHSL(Vi),this.setHSL(Vi.h+e,Vi.s+t,Vi.l+i)}add(e){return this.r+=e.r,this.g+=e.g,this.b+=e.b,this}addColors(e,t){return this.r=e.r+t.r,this.g=e.g+t.g,this.b=e.b+t.b,this}addScalar(e){return this.r+=e,this.g+=e,this.b+=e,this}sub(e){return this.r=Math.max(0,this.r-e.r),this.g=Math.max(0,this.g-e.g),this.b=Math.max(0,this.b-e.b),this}multiply(e){return this.r*=e.r,this.g*=e.g,this.b*=e.b,this}multiplyScalar(e){return this.r*=e,this.g*=e,this.b*=e,this}lerp(e,t){return this.r+=(e.r-this.r)*t,this.g+=(e.g-this.g)*t,this.b+=(e.b-this.b)*t,this}lerpColors(e,t,i){return this.r=e.r+(t.r-e.r)*i,this.g=e.g+(t.g-e.g)*i,this.b=e.b+(t.b-e.b)*i,this}lerpHSL(e,t){this.getHSL(Vi),e.getHSL(el);let i=so(Vi.h,el.h,t),s=so(Vi.s,el.s,t),r=so(Vi.l,el.l,t);return this.setHSL(i,s,r),this}setFromVector3(e){return this.r=e.x,this.g=e.y,this.b=e.z,this}applyMatrix3(e){let t=this.r,i=this.g,s=this.b,r=e.elements;return this.r=r[0]*t+r[3]*i+r[6]*s,this.g=r[1]*t+r[4]*i+r[7]*s,this.b=r[2]*t+r[5]*i+r[8]*s,this}equals(e){return e.r===this.r&&e.g===this.g&&e.b===this.b}fromArray(e,t=0){return this.r=e[t],this.g=e[t+1],this.b=e[t+2],this}toArray(e=[],t=0){return e[t]=this.r,e[t+1]=this.g,e[t+2]=this.b,e}fromBufferAttribute(e,t){return this.r=e.getX(t),this.g=e.getY(t),this.b=e.getZ(t),this}toJSON(){return this.getHex()}*[Symbol.iterator](){yield this.r,yield this.g,yield this.b}},tn=new He;He.NAMES=tm;var mr=class extends Jt{constructor(){super(),this.isScene=!0,this.type="Scene",this.background=null,this.environment=null,this.fog=null,this.backgroundBlurriness=0,this.backgroundIntensity=1,this.backgroundRotation=new Ii,this.environmentIntensity=1,this.environmentRotation=new Ii,this.overrideMaterial=null,typeof __THREE_DEVTOOLS__<"u"&&__THREE_DEVTOOLS__.dispatchEvent(new CustomEvent("observe",{detail:this}))}copy(e,t){return super.copy(e,t),e.background!==null&&(this.background=e.background.clone()),e.environment!==null&&(this.environment=e.environment.clone()),e.fog!==null&&(this.fog=e.fog.clone()),this.backgroundBlurriness=e.backgroundBlurriness,this.backgroundIntensity=e.backgroundIntensity,this.backgroundRotation.copy(e.backgroundRotation),this.environmentIntensity=e.environmentIntensity,this.environmentRotation.copy(e.environmentRotation),e.overrideMaterial!==null&&(this.overrideMaterial=e.overrideMaterial.clone()),this.matrixAutoUpdate=e.matrixAutoUpdate,this}toJSON(e){let t=super.toJSON(e);return this.fog!==null&&(t.object.fog=this.fog.toJSON()),t.object.backgroundBlurriness=this.backgroundBlurriness,t.object.backgroundIntensity=this.backgroundIntensity,t.object.backgroundRotation=this.backgroundRotation.toArray(),t.object.environmentIntensity=this.environmentIntensity,t.object.environmentRotation=this.environmentRotation.toArray(),t}},zn=new L,Ai=new L,Iu=new L,Ti=new L,Js=new L,qs=new L,Up=new L,Bu=new L,Fu=new L,Lu=new L,Uu=new dt,Du=new dt,Pu=new dt,Ri=class n{constructor(e=new L,t=new L,i=new L){this.a=e,this.b=t,this.c=i}static getNormal(e,t,i,s){s.subVectors(i,t),zn.subVectors(e,t),s.cross(zn);let r=s.lengthSq();return r>0?s.multiplyScalar(1/Math.sqrt(r)):s.set(0,0,0)}static getBarycoord(e,t,i,s,r){zn.subVectors(s,t),Ai.subVectors(i,t),Iu.subVectors(e,t);let o=zn.dot(zn),a=zn.dot(Ai),l=zn.dot(Iu),c=Ai.dot(Ai),u=Ai.dot(Iu),d=o*c-a*a;if(d===0)return r.set(0,0,0),null;let h=1/d,f=(c*l-a*u)*h,g=(o*u-a*l)*h;return r.set(1-f-g,g,f)}static containsPoint(e,t,i,s){return this.getBarycoord(e,t,i,s,Ti)===null?!1:Ti.x>=0&&Ti.y>=0&&Ti.x+Ti.y<=1}static getInterpolation(e,t,i,s,r,o,a,l){return this.getBarycoord(e,t,i,s,Ti)===null?(l.x=0,l.y=0,"z"in l&&(l.z=0),"w"in l&&(l.w=0),null):(l.setScalar(0),l.addScaledVector(r,Ti.x),l.addScaledVector(o,Ti.y),l.addScaledVector(a,Ti.z),l)}static getInterpolatedAttribute(e,t,i,s,r,o){return Uu.setScalar(0),Du.setScalar(0),Pu.setScalar(0),Uu.fromBufferAttribute(e,t),Du.fromBufferAttribute(e,i),Pu.fromBufferAttribute(e,s),o.setScalar(0),o.addScaledVector(Uu,r.x),o.addScaledVector(Du,r.y),o.addScaledVector(Pu,r.z),o}static isFrontFacing(e,t,i,s){return zn.subVectors(i,t),Ai.subVectors(e,t),zn.cross(Ai).dot(s)<0}set(e,t,i){return this.a.copy(e),this.b.copy(t),this.c.copy(i),this}setFromPointsAndIndices(e,t,i,s){return this.a.copy(e[t]),this.b.copy(e[i]),this.c.copy(e[s]),this}setFromAttributeAndIndices(e,t,i,s){return this.a.fromBufferAttribute(e,t),this.b.fromBufferAttribute(e,i),this.c.fromBufferAttribute(e,s),this}clone(){return new this.constructor().copy(this)}copy(e){return this.a.copy(e.a),this.b.copy(e.b),this.c.copy(e.c),this}getArea(){return zn.subVectors(this.c,this.b),Ai.subVectors(this.a,this.b),zn.cross(Ai).length()*.5}getMidpoint(e){return e.addVectors(this.a,this.b).add(this.c).multiplyScalar(1/3)}getNormal(e){return n.getNormal(this.a,this.b,this.c,e)}getPlane(e){return e.setFromCoplanarPoints(this.a,this.b,this.c)}getBarycoord(e,t){return n.getBarycoord(e,this.a,this.b,this.c,t)}getInterpolation(e,t,i,s,r){return n.getInterpolation(e,this.a,this.b,this.c,t,i,s,r)}containsPoint(e){return n.containsPoint(e,this.a,this.b,this.c)}isFrontFacing(e){return n.isFrontFacing(this.a,this.b,this.c,e)}intersectsBox(e){return e.intersectsTriangle(this)}closestPointToPoint(e,t){let i=this.a,s=this.b,r=this.c,o,a;Js.subVectors(s,i),qs.subVectors(r,i),Bu.subVectors(e,i);let l=Js.dot(Bu),c=qs.dot(Bu);if(l<=0&&c<=0)return t.copy(i);Fu.subVectors(e,s);let u=Js.dot(Fu),d=qs.dot(Fu);if(u>=0&&d<=u)return t.copy(s);let h=l*d-u*c;if(h<=0&&l>=0&&u<=0)return o=l/(l-u),t.copy(i).addScaledVector(Js,o);Lu.subVectors(e,r);let f=Js.dot(Lu),g=qs.dot(Lu);if(g>=0&&f<=g)return t.copy(r);let y=f*c-l*g;if(y<=0&&c>=0&&g<=0)return a=c/(c-g),t.copy(i).addScaledVector(qs,a);let m=u*g-f*d;if(m<=0&&d-u>=0&&f-g>=0)return Up.subVectors(r,s),a=(d-u)/(d-u+(f-g)),t.copy(s).addScaledVector(Up,a);let p=1/(m+y+h);return o=y*p,a=h*p,t.copy(i).addScaledVector(Js,o).addScaledVector(qs,a)}equals(e){return e.a.equals(this.a)&&e.b.equals(this.b)&&e.c.equals(this.c)}},ci=class{constructor(e=new L(1/0,1/0,1/0),t=new L(-1/0,-1/0,-1/0)){this.isBox3=!0,this.min=e,this.max=t}set(e,t){return this.min.copy(e),this.max.copy(t),this}setFromArray(e){this.makeEmpty();for(let t=0,i=e.length;t<i;t+=3)this.expandByPoint(Wn.fromArray(e,t));return this}setFromBufferAttribute(e){this.makeEmpty();for(let t=0,i=e.count;t<i;t++)this.expandByPoint(Wn.fromBufferAttribute(e,t));return this}setFromPoints(e){this.makeEmpty();for(let t=0,i=e.length;t<i;t++)this.expandByPoint(e[t]);return this}setFromCenterAndSize(e,t){let i=Wn.copy(t).multiplyScalar(.5);return this.min.copy(e).sub(i),this.max.copy(e).add(i),this}setFromObject(e,t=!1){return this.makeEmpty(),this.expandByObject(e,t)}clone(){return new this.constructor().copy(this)}copy(e){return this.min.copy(e.min),this.max.copy(e.max),this}makeEmpty(){return this.min.x=this.min.y=this.min.z=1/0,this.max.x=this.max.y=this.max.z=-1/0,this}isEmpty(){return this.max.x<this.min.x||this.max.y<this.min.y||this.max.z<this.min.z}getCenter(e){return this.isEmpty()?e.set(0,0,0):e.addVectors(this.min,this.max).multiplyScalar(.5)}getSize(e){return this.isEmpty()?e.set(0,0,0):e.subVectors(this.max,this.min)}expandByPoint(e){return this.min.min(e),this.max.max(e),this}expandByVector(e){return this.min.sub(e),this.max.add(e),this}expandByScalar(e){return this.min.addScalar(-e),this.max.addScalar(e),this}expandByObject(e,t=!1){e.updateWorldMatrix(!1,!1);let i=e.geometry;if(i!==void 0){let r=i.getAttribute("position");if(t===!0&&r!==void 0&&e.isInstancedMesh!==!0)for(let o=0,a=r.count;o<a;o++)e.isMesh===!0?e.getVertexPosition(o,Wn):Wn.fromBufferAttribute(r,o),Wn.applyMatrix4(e.matrixWorld),this.expandByPoint(Wn);else e.boundingBox!==void 0?(e.boundingBox===null&&e.computeBoundingBox(),tl.copy(e.boundingBox)):(i.boundingBox===null&&i.computeBoundingBox(),tl.copy(i.boundingBox)),tl.applyMatrix4(e.matrixWorld),this.union(tl)}let s=e.children;for(let r=0,o=s.length;r<o;r++)this.expandByObject(s[r],t);return this}containsPoint(e){return e.x>=this.min.x&&e.x<=this.max.x&&e.y>=this.min.y&&e.y<=this.max.y&&e.z>=this.min.z&&e.z<=this.max.z}containsBox(e){return this.min.x<=e.min.x&&e.max.x<=this.max.x&&this.min.y<=e.min.y&&e.max.y<=this.max.y&&this.min.z<=e.min.z&&e.max.z<=this.max.z}getParameter(e,t){return t.set((e.x-this.min.x)/(this.max.x-this.min.x),(e.y-this.min.y)/(this.max.y-this.min.y),(e.z-this.min.z)/(this.max.z-this.min.z))}intersectsBox(e){return e.max.x>=this.min.x&&e.min.x<=this.max.x&&e.max.y>=this.min.y&&e.min.y<=this.max.y&&e.max.z>=this.min.z&&e.min.z<=this.max.z}intersectsSphere(e){return this.clampPoint(e.center,Wn),Wn.distanceToSquared(e.center)<=e.radius*e.radius}intersectsPlane(e){let t,i;return e.normal.x>0?(t=e.normal.x*this.min.x,i=e.normal.x*this.max.x):(t=e.normal.x*this.max.x,i=e.normal.x*this.min.x),e.normal.y>0?(t+=e.normal.y*this.min.y,i+=e.normal.y*this.max.y):(t+=e.normal.y*this.max.y,i+=e.normal.y*this.min.y),e.normal.z>0?(t+=e.normal.z*this.min.z,i+=e.normal.z*this.max.z):(t+=e.normal.z*this.max.z,i+=e.normal.z*this.min.z),t<=-e.constant&&i>=-e.constant}intersectsTriangle(e){if(this.isEmpty())return!1;this.getCenter(jr),nl.subVectors(this.max,jr),Qs.subVectors(e.a,jr),js.subVectors(e.b,jr),er.subVectors(e.c,jr),Oi.subVectors(js,Qs),ki.subVectors(er,js),os.subVectors(Qs,er);let t=[0,-Oi.z,Oi.y,0,-ki.z,ki.y,0,-os.z,os.y,Oi.z,0,-Oi.x,ki.z,0,-ki.x,os.z,0,-os.x,-Oi.y,Oi.x,0,-ki.y,ki.x,0,-os.y,os.x,0];return!Gu(t,Qs,js,er,nl)||(t=[1,0,0,0,1,0,0,0,1],!Gu(t,Qs,js,er,nl))?!1:(il.crossVectors(Oi,ki),t=[il.x,il.y,il.z],Gu(t,Qs,js,er,nl))}clampPoint(e,t){return t.copy(e).clamp(this.min,this.max)}distanceToPoint(e){return this.clampPoint(e,Wn).distanceTo(e)}getBoundingSphere(e){return this.isEmpty()?e.makeEmpty():(this.getCenter(e.center),e.radius=this.getSize(Wn).length()*.5),e}intersect(e){return this.min.max(e.min),this.max.min(e.max),this.isEmpty()&&this.makeEmpty(),this}union(e){return this.min.min(e.min),this.max.max(e.max),this}applyMatrix4(e){return this.isEmpty()?this:(Ei[0].set(this.min.x,this.min.y,this.min.z).applyMatrix4(e),Ei[1].set(this.min.x,this.min.y,this.max.z).applyMatrix4(e),Ei[2].set(this.min.x,this.max.y,this.min.z).applyMatrix4(e),Ei[3].set(this.min.x,this.max.y,this.max.z).applyMatrix4(e),Ei[4].set(this.max.x,this.min.y,this.min.z).applyMatrix4(e),Ei[5].set(this.max.x,this.min.y,this.max.z).applyMatrix4(e),Ei[6].set(this.max.x,this.max.y,this.min.z).applyMatrix4(e),Ei[7].set(this.max.x,this.max.y,this.max.z).applyMatrix4(e),this.setFromPoints(Ei),this)}translate(e){return this.min.add(e),this.max.add(e),this}equals(e){return e.min.equals(this.min)&&e.max.equals(this.max)}toJSON(){return{min:this.min.toArray(),max:this.max.toArray()}}fromJSON(e){return this.min.fromArray(e.min),this.max.fromArray(e.max),this}},Ei=[new L,new L,new L,new L,new L,new L,new L,new L],Wn=new L,tl=new ci,Qs=new L,js=new L,er=new L,Oi=new L,ki=new L,os=new L,jr=new L,nl=new L,il=new L,as=new L;function Gu(n,e,t,i,s){for(let r=0,o=n.length-3;r<=o;r+=3){as.fromArray(n,r);let a=s.x*Math.abs(as.x)+s.y*Math.abs(as.y)+s.z*Math.abs(as.z),l=e.dot(as),c=t.dot(as),u=i.dot(as);if(Math.max(-Math.max(l,c,u),Math.min(l,c,u))>a)return!1}return!0}var wi=ey();function ey(){let n=new ArrayBuffer(4),e=new Float32Array(n),t=new Uint32Array(n),i=new Uint32Array(512),s=new Uint32Array(512);for(let l=0;l<256;++l){let c=l-127;c<-27?(i[l]=0,i[l|256]=32768,s[l]=24,s[l|256]=24):c<-14?(i[l]=1024>>-c-14,i[l|256]=1024>>-c-14|32768,s[l]=-c-1,s[l|256]=-c-1):c<=15?(i[l]=c+15<<10,i[l|256]=c+15<<10|32768,s[l]=13,s[l|256]=13):c<128?(i[l]=31744,i[l|256]=64512,s[l]=24,s[l|256]=24):(i[l]=31744,i[l|256]=64512,s[l]=13,s[l|256]=13)}let r=new Uint32Array(2048),o=new Uint32Array(64),a=new Uint32Array(64);for(let l=1;l<1024;++l){let c=l<<13,u=0;for(;(c&8388608)===0;)c<<=1,u-=8388608;c&=-8388609,u+=947912704,r[l]=c|u}for(let l=1024;l<2048;++l)r[l]=939524096+(l-1024<<13);for(let l=1;l<31;++l)o[l]=l<<23;o[31]=1199570944,o[32]=2147483648;for(let l=33;l<63;++l)o[l]=2147483648+(l-32<<23);o[63]=3347054592;for(let l=1;l<64;++l)l!==32&&(a[l]=1024);return{floatView:e,uint32View:t,baseTable:i,shiftTable:s,mantissaTable:r,exponentTable:o,offsetTable:a}}function ty(n){Math.abs(n)>65504&&pe("DataUtils.toHalfFloat(): Value out of range."),n=qe(n,-65504,65504),wi.floatView[0]=n;let e=wi.uint32View[0],t=e>>23&511;return wi.baseTable[t]+((e&8388607)>>wi.shiftTable[t])}function ny(n){let e=n>>10;return wi.uint32View[0]=wi.mantissaTable[wi.offsetTable[e]+(n&1023)]+wi.exponentTable[e],wi.floatView[0]}var mo=class{static toHalfFloat(e){return ty(e)}static fromHalfFloat(e){return ny(e)}},Ut=new L,sl=new ze,iy=0,Xt=class extends wn{constructor(e,t,i=!1){if(super(),Array.isArray(e))throw new TypeError("THREE.BufferAttribute: array should be a Typed Array.");this.isBufferAttribute=!0,Object.defineProperty(this,"id",{value:iy++}),this.name="",this.array=e,this.itemSize=t,this.count=e!==void 0?e.length/t:0,this.normalized=i,this.usage=Lr,this.updateRanges=[],this.gpuType=Dn,this.version=0}onUploadCallback(){}set needsUpdate(e){e===!0&&this.version++}setUsage(e){return this.usage=e,this}addUpdateRange(e,t){this.updateRanges.push({start:e,count:t})}clearUpdateRanges(){this.updateRanges.length=0}copy(e){return this.name=e.name,this.array=new e.array.constructor(e.array),this.itemSize=e.itemSize,this.count=e.count,this.normalized=e.normalized,this.usage=e.usage,this.gpuType=e.gpuType,this}copyAt(e,t,i){e*=this.itemSize,i*=t.itemSize;for(let s=0,r=this.itemSize;s<r;s++)this.array[e+s]=t.array[i+s];return this}copyArray(e){return this.array.set(e),this}applyMatrix3(e){if(this.itemSize===2)for(let t=0,i=this.count;t<i;t++)sl.fromBufferAttribute(this,t),sl.applyMatrix3(e),this.setXY(t,sl.x,sl.y);else if(this.itemSize===3)for(let t=0,i=this.count;t<i;t++)Ut.fromBufferAttribute(this,t),Ut.applyMatrix3(e),this.setXYZ(t,Ut.x,Ut.y,Ut.z);return this}applyMatrix4(e){for(let t=0,i=this.count;t<i;t++)Ut.fromBufferAttribute(this,t),Ut.applyMatrix4(e),this.setXYZ(t,Ut.x,Ut.y,Ut.z);return this}applyNormalMatrix(e){for(let t=0,i=this.count;t<i;t++)Ut.fromBufferAttribute(this,t),Ut.applyNormalMatrix(e),this.setXYZ(t,Ut.x,Ut.y,Ut.z);return this}transformDirection(e){for(let t=0,i=this.count;t<i;t++)Ut.fromBufferAttribute(this,t),Ut.transformDirection(e),this.setXYZ(t,Ut.x,Ut.y,Ut.z);return this}set(e,t=0){return this.array.set(e,t),this}getComponent(e,t){let i=this.array[e*this.itemSize+t];return this.normalized&&(i=Hn(i,this.array)),i}setComponent(e,t,i){return this.normalized&&(i=ht(i,this.array)),this.array[e*this.itemSize+t]=i,this}getX(e){let t=this.array[e*this.itemSize];return this.normalized&&(t=Hn(t,this.array)),t}setX(e,t){return this.normalized&&(t=ht(t,this.array)),this.array[e*this.itemSize]=t,this}getY(e){let t=this.array[e*this.itemSize+1];return this.normalized&&(t=Hn(t,this.array)),t}setY(e,t){return this.normalized&&(t=ht(t,this.array)),this.array[e*this.itemSize+1]=t,this}getZ(e){let t=this.array[e*this.itemSize+2];return this.normalized&&(t=Hn(t,this.array)),t}setZ(e,t){return this.normalized&&(t=ht(t,this.array)),this.array[e*this.itemSize+2]=t,this}getW(e){let t=this.array[e*this.itemSize+3];return this.normalized&&(t=Hn(t,this.array)),t}setW(e,t){return this.normalized&&(t=ht(t,this.array)),this.array[e*this.itemSize+3]=t,this}setXY(e,t,i){return e*=this.itemSize,this.normalized&&(t=ht(t,this.array),i=ht(i,this.array)),this.array[e+0]=t,this.array[e+1]=i,this}setXYZ(e,t,i,s){return e*=this.itemSize,this.normalized&&(t=ht(t,this.array),i=ht(i,this.array),s=ht(s,this.array)),this.array[e+0]=t,this.array[e+1]=i,this.array[e+2]=s,this}setXYZW(e,t,i,s,r){return e*=this.itemSize,this.normalized&&(t=ht(t,this.array),i=ht(i,this.array),s=ht(s,this.array),r=ht(r,this.array)),this.array[e+0]=t,this.array[e+1]=i,this.array[e+2]=s,this.array[e+3]=r,this}onUpload(e){return this.onUploadCallback=e,this}clone(){return new this.constructor(this.array,this.itemSize).copy(this)}toJSON(){let e={itemSize:this.itemSize,type:this.array.constructor.name,array:Array.from(this.array),normalized:this.normalized};return e.name=this.name,e.usage=this.usage,e.gpuType=this.gpuType,e}dispose(){this.dispatchEvent({type:"dispose"})}};var gr=class extends Xt{constructor(e,t,i){super(new Uint16Array(e),t,i)}};var xr=class extends Xt{constructor(e,t,i){super(new Uint32Array(e),t,i)}};var nn=class extends Xt{constructor(e,t,i){super(new Float32Array(e),t,i)}},sy=new ci,eo=new L,Vu=new L,Kn=class{constructor(e=new L,t=-1){this.isSphere=!0,this.center=e,this.radius=t}set(e,t){return this.center.copy(e),this.radius=t,this}setFromPoints(e,t){let i=this.center;t!==void 0?i.copy(t):sy.setFromPoints(e).getCenter(i);let s=0;for(let r=0,o=e.length;r<o;r++)s=Math.max(s,i.distanceToSquared(e[r]));return this.radius=Math.sqrt(s),this}copy(e){return this.center.copy(e.center),this.radius=e.radius,this}isEmpty(){return this.radius<0}makeEmpty(){return this.center.set(0,0,0),this.radius=-1,this}containsPoint(e){return e.distanceToSquared(this.center)<=this.radius*this.radius}distanceToPoint(e){return e.distanceTo(this.center)-this.radius}intersectsSphere(e){let t=this.radius+e.radius;return e.center.distanceToSquared(this.center)<=t*t}intersectsBox(e){return e.intersectsSphere(this)}intersectsPlane(e){return Math.abs(e.distanceToPoint(this.center))<=this.radius}clampPoint(e,t){let i=this.center.distanceToSquared(e);return t.copy(e),i>this.radius*this.radius&&(t.sub(this.center).normalize(),t.multiplyScalar(this.radius).add(this.center)),t}getBoundingBox(e){return this.isEmpty()?(e.makeEmpty(),e):(e.set(this.center,this.center),e.expandByScalar(this.radius),e)}applyMatrix4(e){return this.center.applyMatrix4(e),this.radius=this.radius*e.getMaxScaleOnAxis(),this}translate(e){return this.center.add(e),this}expandByPoint(e){if(this.isEmpty())return this.center.copy(e),this.radius=0,this;eo.subVectors(e,this.center);let t=eo.lengthSq();if(t>this.radius*this.radius){let i=Math.sqrt(t),s=(i-this.radius)*.5;this.center.addScaledVector(eo,s/i),this.radius+=s}return this}union(e){return e.isEmpty()?this:this.isEmpty()?(this.copy(e),this):(this.center.equals(e.center)===!0?this.radius=Math.max(this.radius,e.radius):(Vu.subVectors(e.center,this.center).setLength(e.radius),this.expandByPoint(eo.copy(e.center).add(Vu)),this.expandByPoint(eo.copy(e.center).sub(Vu))),this)}equals(e){return e.center.equals(this.center)&&e.radius===this.radius}clone(){return new this.constructor().copy(this)}toJSON(){return{radius:this.radius,center:this.center.toArray()}}fromJSON(e){return this.radius=e.radius,this.center.fromArray(e.center),this}},ry=0,Fn=new ct,Ou=new Jt,tr=new L,Mn=new ci,to=new ci,Zt=new L,dn=class n extends wn{constructor(){super(),this.isBufferGeometry=!0,Object.defineProperty(this,"id",{value:ry++}),this.uuid=Ci(),this.name="",this.type="BufferGeometry",this.index=null,this.indirect=null,this.indirectOffset=0,this.attributes={},this.morphAttributes={},this.morphTargetsRelative=!1,this.groups=[],this.boundingBox=null,this.boundingSphere=null,this.drawRange={start:0,count:1/0},this.userData={},this._transformed=!1}getIndex(){return this.index}setIndex(e){return Array.isArray(e)?this.index=new(R0(e)?xr:gr)(e,1):this.index=e,this}setIndirect(e,t=0){return this.indirect=e,this.indirectOffset=t,this}getIndirect(){return this.indirect}getAttribute(e){return this.attributes[e]}setAttribute(e,t){return this.attributes[e]=t,this}deleteAttribute(e){return delete this.attributes[e],this}hasAttribute(e){return this.attributes[e]!==void 0}addGroup(e,t,i=0){this.groups.push({start:e,count:t,materialIndex:i})}clearGroups(){this.groups=[]}setDrawRange(e,t){this.drawRange.start=e,this.drawRange.count=t}applyMatrix4(e){let t=this.attributes.position;t!==void 0&&(t.applyMatrix4(e),t.needsUpdate=!0);let i=this.attributes.normal;if(i!==void 0){let r=new Ge().getNormalMatrix(e);i.applyNormalMatrix(r),i.needsUpdate=!0}let s=this.attributes.tangent;return s!==void 0&&(s.transformDirection(e),s.needsUpdate=!0),this.boundingBox!==null&&this.computeBoundingBox(),this.boundingSphere!==null&&this.computeBoundingSphere(),this._transformed=!0,this}applyQuaternion(e){return Fn.makeRotationFromQuaternion(e),this.applyMatrix4(Fn),this}rotateX(e){return Fn.makeRotationX(e),this.applyMatrix4(Fn),this}rotateY(e){return Fn.makeRotationY(e),this.applyMatrix4(Fn),this}rotateZ(e){return Fn.makeRotationZ(e),this.applyMatrix4(Fn),this}translate(e,t,i){return Fn.makeTranslation(e,t,i),this.applyMatrix4(Fn),this}scale(e,t,i){return Fn.makeScale(e,t,i),this.applyMatrix4(Fn),this}lookAt(e){return Ou.lookAt(e),Ou.updateMatrix(),this.applyMatrix4(Ou.matrix),this}center(){return this.computeBoundingBox(),this.boundingBox.getCenter(tr).negate(),this.translate(tr.x,tr.y,tr.z),this}setFromPoints(e){let t=this.getAttribute("position");if(t===void 0){let i=[];for(let s=0,r=e.length;s<r;s++){let o=e[s];i.push(o.x,o.y,o.z||0)}this.setAttribute("position",new nn(i,3))}else{let i=Math.min(e.length,t.count);for(let s=0;s<i;s++){let r=e[s];t.setXYZ(s,r.x,r.y,r.z||0)}e.length>t.count&&pe("BufferGeometry: Buffer size too small for points data. Use .dispose() and create a new geometry."),t.needsUpdate=!0}return this}computeBoundingBox(){this.boundingBox===null&&(this.boundingBox=new ci);let e=this.attributes.position,t=this.morphAttributes.position;if(e&&e.isGLBufferAttribute){_e("BufferGeometry.computeBoundingBox(): GLBufferAttribute requires a manual bounding box.",this),this.boundingBox.set(new L(-1/0,-1/0,-1/0),new L(1/0,1/0,1/0));return}if(e!==void 0){if(this.boundingBox.setFromBufferAttribute(e),t)for(let i=0,s=t.length;i<s;i++){let r=t[i];Mn.setFromBufferAttribute(r),this.morphTargetsRelative?(Zt.addVectors(this.boundingBox.min,Mn.min),this.boundingBox.expandByPoint(Zt),Zt.addVectors(this.boundingBox.max,Mn.max),this.boundingBox.expandByPoint(Zt)):(this.boundingBox.expandByPoint(Mn.min),this.boundingBox.expandByPoint(Mn.max))}}else this.boundingBox.makeEmpty();(isNaN(this.boundingBox.min.x)||isNaN(this.boundingBox.min.y)||isNaN(this.boundingBox.min.z))&&_e('BufferGeometry.computeBoundingBox(): Computed min/max have NaN values. The "position" attribute is likely to have NaN values.',this)}computeBoundingSphere(){this.boundingSphere===null&&(this.boundingSphere=new Kn);let e=this.attributes.position,t=this.morphAttributes.position;if(e&&e.isGLBufferAttribute){_e("BufferGeometry.computeBoundingSphere(): GLBufferAttribute requires a manual bounding sphere.",this),this.boundingSphere.set(new L,1/0);return}if(e){let i=this.boundingSphere.center;if(Mn.setFromBufferAttribute(e),t)for(let r=0,o=t.length;r<o;r++){let a=t[r];to.setFromBufferAttribute(a),this.morphTargetsRelative?(Zt.addVectors(Mn.min,to.min),Mn.expandByPoint(Zt),Zt.addVectors(Mn.max,to.max),Mn.expandByPoint(Zt)):(Mn.expandByPoint(to.min),Mn.expandByPoint(to.max))}Mn.getCenter(i);let s=0;for(let r=0,o=e.count;r<o;r++)Zt.fromBufferAttribute(e,r),s=Math.max(s,i.distanceToSquared(Zt));if(t)for(let r=0,o=t.length;r<o;r++){let a=t[r],l=this.morphTargetsRelative;for(let c=0,u=a.count;c<u;c++)Zt.fromBufferAttribute(a,c),l&&(tr.fromBufferAttribute(e,c),Zt.add(tr)),s=Math.max(s,i.distanceToSquared(Zt))}this.boundingSphere.radius=Math.sqrt(s),isNaN(this.boundingSphere.radius)&&_e('BufferGeometry.computeBoundingSphere(): Computed radius is NaN. The "position" attribute is likely to have NaN values.',this)}}computeTangents(){let e=this.index,t=this.attributes;if(e===null||t.position===void 0||t.normal===void 0||t.uv===void 0){_e("BufferGeometry: .computeTangents() failed. Missing required attributes (index, position, normal or uv)");return}let i=t.position,s=t.normal,r=t.uv,o=this.getAttribute("tangent");(o===void 0||o.count!==i.count)&&(o=new Xt(new Float32Array(4*i.count),4),this.setAttribute("tangent",o));let a=[],l=[];for(let b=0;b<i.count;b++)a[b]=new L,l[b]=new L;let c=new L,u=new L,d=new L,h=new ze,f=new ze,g=new ze,y=new L,m=new L;function p(b,M,B){c.fromBufferAttribute(i,b),u.fromBufferAttribute(i,M),d.fromBufferAttribute(i,B),h.fromBufferAttribute(r,b),f.fromBufferAttribute(r,M),g.fromBufferAttribute(r,B),u.sub(c),d.sub(c),f.sub(h),g.sub(h);let D=1/(f.x*g.y-g.x*f.y);isFinite(D)&&(y.copy(u).multiplyScalar(g.y).addScaledVector(d,-f.y).multiplyScalar(D),m.copy(d).multiplyScalar(f.x).addScaledVector(u,-g.x).multiplyScalar(D),a[b].add(y),a[M].add(y),a[B].add(y),l[b].add(m),l[M].add(m),l[B].add(m))}let E=this.groups;E.length===0&&(E=[{start:0,count:e.count}]);for(let b=0,M=E.length;b<M;++b){let B=E[b],D=B.start,k=B.count;for(let H=D,F=D+k;H<F;H+=3)p(e.getX(H+0),e.getX(H+1),e.getX(H+2))}let w=new L,S=new L,v=new L,A=new L;function R(b){v.fromBufferAttribute(s,b),A.copy(v);let M=a[b];w.copy(M),w.sub(v.multiplyScalar(v.dot(M))).normalize(),S.crossVectors(A,M);let D=S.dot(l[b])<0?-1:1;o.setXYZW(b,w.x,w.y,w.z,D)}for(let b=0,M=E.length;b<M;++b){let B=E[b],D=B.start,k=B.count;for(let H=D,F=D+k;H<F;H+=3)R(e.getX(H+0)),R(e.getX(H+1)),R(e.getX(H+2))}this._transformed=!0}computeVertexNormals(){let e=this.index,t=this.getAttribute("position");if(t!==void 0){let i=this.getAttribute("normal");if(i===void 0||i.count!==t.count)i=new Xt(new Float32Array(t.count*3),3),this.setAttribute("normal",i);else for(let h=0,f=i.count;h<f;h++)i.setXYZ(h,0,0,0);let s=new L,r=new L,o=new L,a=new L,l=new L,c=new L,u=new L,d=new L;if(e)for(let h=0,f=e.count;h<f;h+=3){let g=e.getX(h+0),y=e.getX(h+1),m=e.getX(h+2);s.fromBufferAttribute(t,g),r.fromBufferAttribute(t,y),o.fromBufferAttribute(t,m),u.subVectors(o,r),d.subVectors(s,r),u.cross(d),a.fromBufferAttribute(i,g),l.fromBufferAttribute(i,y),c.fromBufferAttribute(i,m),a.add(u),l.add(u),c.add(u),i.setXYZ(g,a.x,a.y,a.z),i.setXYZ(y,l.x,l.y,l.z),i.setXYZ(m,c.x,c.y,c.z)}else for(let h=0,f=t.count;h<f;h+=3)s.fromBufferAttribute(t,h+0),r.fromBufferAttribute(t,h+1),o.fromBufferAttribute(t,h+2),u.subVectors(o,r),d.subVectors(s,r),u.cross(d),i.setXYZ(h+0,u.x,u.y,u.z),i.setXYZ(h+1,u.x,u.y,u.z),i.setXYZ(h+2,u.x,u.y,u.z);this.normalizeNormals(),i.needsUpdate=!0}}normalizeNormals(){let e=this.attributes.normal;for(let t=0,i=e.count;t<i;t++)Zt.fromBufferAttribute(e,t),Zt.normalize(),e.setXYZ(t,Zt.x,Zt.y,Zt.z)}toNonIndexed(){function e(a,l){let c=a.array,u=a.itemSize,d=a.normalized,h=new c.constructor(l.length*u),f=0,g=0;for(let y=0,m=l.length;y<m;y++){a.isInterleavedBufferAttribute?f=l[y]*a.data.stride+a.offset:f=l[y]*u;for(let p=0;p<u;p++)h[g++]=c[f++]}return new Xt(h,u,d)}if(this.index===null)return pe("BufferGeometry.toNonIndexed(): BufferGeometry is already non-indexed."),this;let t=new n,i=this.index.array,s=this.attributes;for(let a in s){let l=s[a],c=e(l,i);t.setAttribute(a,c)}let r=this.morphAttributes;for(let a in r){let l=[],c=r[a];for(let u=0,d=c.length;u<d;u++){let h=c[u],f=e(h,i);l.push(f)}t.morphAttributes[a]=l}t.morphTargetsRelative=this.morphTargetsRelative;let o=this.groups;for(let a=0,l=o.length;a<l;a++){let c=o[a];t.addGroup(c.start,c.count,c.materialIndex)}return t}toJSON(){let e={metadata:{version:4.7,type:"BufferGeometry",generator:"BufferGeometry.toJSON"}};if(e.uuid=this.uuid,e.type=this.parameters!==void 0&&this._transformed===!0?"BufferGeometry":this.type,e.name=this.name,Object.keys(this.userData).length>0&&(e.userData=this.userData),this.parameters!==void 0&&this._transformed!==!0){let l=this.parameters;for(let c in l)l[c]!==void 0&&(e[c]=l[c]);return e}e.data={attributes:{}};let t=this.index;t!==null&&(e.data.index={type:t.array.constructor.name,array:Array.prototype.slice.call(t.array)});let i=this.attributes;for(let l in i){let c=i[l];e.data.attributes[l]=c.toJSON(e.data)}let s={},r=!1;for(let l in this.morphAttributes){let c=this.morphAttributes[l],u=[];for(let d=0,h=c.length;d<h;d++){let f=c[d];u.push(f.toJSON(e.data))}u.length>0&&(s[l]=u,r=!0)}r&&(e.data.morphAttributes=s,e.data.morphTargetsRelative=this.morphTargetsRelative);let o=this.groups;o.length>0&&(e.data.groups=JSON.parse(JSON.stringify(o)));let a=this.boundingSphere;return a!==null&&(e.data.boundingSphere=a.toJSON()),e}clone(){return new this.constructor().copy(this)}copy(e){this.index=null,this.attributes={},this.morphAttributes={},this.groups=[],this.boundingBox=null,this.boundingSphere=null;let t={};this.name=e.name;let i=e.index;i!==null&&this.setIndex(i.clone());let s=e.attributes;for(let c in s){let u=s[c];this.setAttribute(c,u.clone(t))}let r=e.morphAttributes;for(let c in r){let u=[],d=r[c];for(let h=0,f=d.length;h<f;h++)u.push(d[h].clone(t));this.morphAttributes[c]=u}this.morphTargetsRelative=e.morphTargetsRelative;let o=e.groups;for(let c=0,u=o.length;c<u;c++){let d=o[c];this.addGroup(d.start,d.count,d.materialIndex)}let a=e.boundingBox;a!==null&&(this.boundingBox=a.clone());let l=e.boundingSphere;return l!==null&&(this.boundingSphere=l.clone()),this.drawRange.start=e.drawRange.start,this.drawRange.count=e.drawRange.count,this.userData=e.userData,this._transformed=e._transformed,this}dispose(){this.dispatchEvent({type:"dispose"})}},go=class{constructor(e,t){this.isInterleavedBuffer=!0,this.array=e,this.stride=t,this.count=e!==void 0?e.length/t:0,this.usage=Lr,this.updateRanges=[],this.version=0,this.uuid=Ci()}onUploadCallback(){}set needsUpdate(e){e===!0&&this.version++}setUsage(e){return this.usage=e,this}addUpdateRange(e,t){this.updateRanges.push({start:e,count:t})}clearUpdateRanges(){this.updateRanges.length=0}copy(e){return this.array=new e.array.constructor(e.array),this.count=e.count,this.stride=e.stride,this.usage=e.usage,this}copyAt(e,t,i){e*=this.stride,i*=t.stride;for(let s=0,r=this.stride;s<r;s++)this.array[e+s]=t.array[i+s];return this}set(e,t=0){return this.array.set(e,t),this}clone(e){e.arrayBuffers===void 0&&(e.arrayBuffers={}),this.array.buffer._uuid===void 0&&(this.array.buffer._uuid=Ci()),e.arrayBuffers[this.array.buffer._uuid]===void 0&&(e.arrayBuffers[this.array.buffer._uuid]=this.array.slice(0).buffer);let t=new this.array.constructor(e.arrayBuffers[this.array.buffer._uuid]),i=new this.constructor(t,this.stride);return i.setUsage(this.usage),i}onUpload(e){return this.onUploadCallback=e,this}toJSON(e){e.arrayBuffers===void 0&&(e.arrayBuffers={}),this.array.buffer._uuid===void 0&&(this.array.buffer._uuid=Ci()),e.arrayBuffers[this.array.buffer._uuid]===void 0&&(e.arrayBuffers[this.array.buffer._uuid]=Array.from(new Uint32Array(this.array.buffer)));let t={uuid:this.uuid,buffer:this.array.buffer._uuid,type:this.array.constructor.name,stride:this.stride};return t.usage=this.usage,t}},un=new L,xo=class n{constructor(e,t,i,s=!1){this.isInterleavedBufferAttribute=!0,this.name="",this.data=e,this.itemSize=t,this.offset=i,this.normalized=s}get count(){return this.data.count}get array(){return this.data.array}set needsUpdate(e){this.data.needsUpdate=e}applyMatrix4(e){for(let t=0,i=this.data.count;t<i;t++)un.fromBufferAttribute(this,t),un.applyMatrix4(e),this.setXYZ(t,un.x,un.y,un.z);return this}applyNormalMatrix(e){for(let t=0,i=this.count;t<i;t++)un.fromBufferAttribute(this,t),un.applyNormalMatrix(e),this.setXYZ(t,un.x,un.y,un.z);return this}transformDirection(e){for(let t=0,i=this.count;t<i;t++)un.fromBufferAttribute(this,t),un.transformDirection(e),this.setXYZ(t,un.x,un.y,un.z);return this}getComponent(e,t){let i=this.array[e*this.data.stride+this.offset+t];return this.normalized&&(i=Hn(i,this.array)),i}setComponent(e,t,i){return this.normalized&&(i=ht(i,this.array)),this.data.array[e*this.data.stride+this.offset+t]=i,this}setX(e,t){return this.normalized&&(t=ht(t,this.array)),this.data.array[e*this.data.stride+this.offset]=t,this}setY(e,t){return this.normalized&&(t=ht(t,this.array)),this.data.array[e*this.data.stride+this.offset+1]=t,this}setZ(e,t){return this.normalized&&(t=ht(t,this.array)),this.data.array[e*this.data.stride+this.offset+2]=t,this}setW(e,t){return this.normalized&&(t=ht(t,this.array)),this.data.array[e*this.data.stride+this.offset+3]=t,this}getX(e){let t=this.data.array[e*this.data.stride+this.offset];return this.normalized&&(t=Hn(t,this.array)),t}getY(e){let t=this.data.array[e*this.data.stride+this.offset+1];return this.normalized&&(t=Hn(t,this.array)),t}getZ(e){let t=this.data.array[e*this.data.stride+this.offset+2];return this.normalized&&(t=Hn(t,this.array)),t}getW(e){let t=this.data.array[e*this.data.stride+this.offset+3];return this.normalized&&(t=Hn(t,this.array)),t}setXY(e,t,i){return e=e*this.data.stride+this.offset,this.normalized&&(t=ht(t,this.array),i=ht(i,this.array)),this.data.array[e+0]=t,this.data.array[e+1]=i,this}setXYZ(e,t,i,s){return e=e*this.data.stride+this.offset,this.normalized&&(t=ht(t,this.array),i=ht(i,this.array),s=ht(s,this.array)),this.data.array[e+0]=t,this.data.array[e+1]=i,this.data.array[e+2]=s,this}setXYZW(e,t,i,s,r){return e=e*this.data.stride+this.offset,this.normalized&&(t=ht(t,this.array),i=ht(i,this.array),s=ht(s,this.array),r=ht(r,this.array)),this.data.array[e+0]=t,this.data.array[e+1]=i,this.data.array[e+2]=s,this.data.array[e+3]=r,this}clone(e){if(e===void 0){hs("InterleavedBufferAttribute.clone(): Cloning an interleaved buffer attribute will de-interleave buffer data.");let t=[];for(let i=0;i<this.count;i++){let s=i*this.data.stride+this.offset;for(let r=0;r<this.itemSize;r++)t.push(this.data.array[s+r])}return new Xt(new this.array.constructor(t),this.itemSize,this.normalized)}else return e.interleavedBuffers===void 0&&(e.interleavedBuffers={}),e.interleavedBuffers[this.data.uuid]===void 0&&(e.interleavedBuffers[this.data.uuid]=this.data.clone(e)),new n(e.interleavedBuffers[this.data.uuid],this.itemSize,this.offset,this.normalized)}toJSON(e){if(e===void 0){hs("InterleavedBufferAttribute.toJSON(): Serializing an interleaved buffer attribute will de-interleave buffer data.");let t=[];for(let i=0;i<this.count;i++){let s=i*this.data.stride+this.offset;for(let r=0;r<this.itemSize;r++)t.push(this.data.array[s+r])}return{itemSize:this.itemSize,type:this.array.constructor.name,array:t,normalized:this.normalized}}else return e.interleavedBuffers===void 0&&(e.interleavedBuffers={}),e.interleavedBuffers[this.data.uuid]===void 0&&(e.interleavedBuffers[this.data.uuid]=this.data.toJSON(e)),{isInterleavedBufferAttribute:!0,itemSize:this.itemSize,data:this.data.uuid,offset:this.offset,normalized:this.normalized}}},ku=new L,oy=new L,ay=new Ge,Rn=class{constructor(e=new L(1,0,0),t=0){this.isPlane=!0,this.normal=e,this.constant=t}set(e,t){return this.normal.copy(e),this.constant=t,this}setComponents(e,t,i,s){return this.normal.set(e,t,i),this.constant=s,this}setFromNormalAndCoplanarPoint(e,t){return this.normal.copy(e),this.constant=-t.dot(this.normal),this}setFromCoplanarPoints(e,t,i){let s=ku.subVectors(i,t).cross(oy.subVectors(e,t)).normalize();return this.setFromNormalAndCoplanarPoint(s,e),this}copy(e){return this.normal.copy(e.normal),this.constant=e.constant,this}normalize(){let e=1/this.normal.length();return this.normal.multiplyScalar(e),this.constant*=e,this}negate(){return this.constant*=-1,this.normal.negate(),this}distanceToPoint(e){return this.normal.dot(e)+this.constant}distanceToSphere(e){return this.distanceToPoint(e.center)-e.radius}projectPoint(e,t){return t.copy(e).addScaledVector(this.normal,-this.distanceToPoint(e))}intersectLine(e,t,i=!0){let s=e.delta(ku),r=this.normal.dot(s);if(r===0)return this.distanceToPoint(e.start)===0?t.copy(e.start):null;let o=-(e.start.dot(this.normal)+this.constant)/r;return i===!0&&(o<0||o>1)?null:t.copy(e.start).addScaledVector(s,o)}intersectsLine(e){let t=this.distanceToPoint(e.start),i=this.distanceToPoint(e.end);return t<0&&i>0||i<0&&t>0}intersectsBox(e){return e.intersectsPlane(this)}intersectsSphere(e){return e.intersectsPlane(this)}coplanarPoint(e){return e.copy(this.normal).multiplyScalar(-this.constant)}applyMatrix4(e,t){let i=t||ay.getNormalMatrix(e),s=this.coplanarPoint(ku).applyMatrix4(e),r=this.normal.applyMatrix3(i).normalize();return this.constant=-s.dot(r),this}translate(e){return this.constant-=e.dot(this.normal),this}equals(e){return e.normal.equals(this.normal)&&e.constant===this.constant}clone(){return new this.constructor().copy(this)}toJSON(){return{normal:this.normal.toArray(),constant:this.constant}}fromJSON(e){return this.normal.fromArray(e.normal),this.constant=e.constant,this}},ly=0,ui=class extends wn{constructor(){super(),this.isMaterial=!0,Object.defineProperty(this,"id",{value:ly++}),this.uuid=Ci(),this.name="",this.type="Material",this.blending=Ss,this.side=Jn,this.vertexColors=!1,this.opacity=1,this.transparent=!1,this.alphaHash=!1,this.blendSrc=Xl,this.blendDst=Kl,this.blendEquation=Xi,this.blendSrcAlpha=null,this.blendDstAlpha=null,this.blendEquationAlpha=null,this.blendColor=new He(0,0,0),this.blendAlpha=0,this.depthFunc=us,this.depthTest=!0,this.depthWrite=!0,this.stencilWriteMask=255,this.stencilFunc=wh,this.stencilRef=0,this.stencilFuncMask=255,this.stencilFail=io,this.stencilZFail=io,this.stencilZPass=io,this.stencilWrite=!1,this.clippingPlanes=null,this.clipIntersection=!1,this.clipShadows=!1,this.shadowSide=null,this.colorWrite=!0,this.precision=null,this.polygonOffset=!1,this.polygonOffsetFactor=0,this.polygonOffsetUnits=0,this.dithering=!1,this.alphaToCoverage=!1,this.premultipliedAlpha=!1,this.forceSinglePass=!1,this.allowOverride=!0,this.visible=!0,this.toneMapped=!0,this.userData={},this.version=0,this._alphaTest=0}get alphaTest(){return this._alphaTest}set alphaTest(e){this._alphaTest>0!=e>0&&this.version++,this._alphaTest=e}onBeforeRender(){}onBeforeCompile(){}customProgramCacheKey(){return this.onBeforeCompile.toString()}setValues(e){if(e!==void 0)for(let t in e){let i=e[t];if(i===void 0){pe(`Material: parameter '${t}' has value of undefined.`);continue}let s=this[t];if(s===void 0){pe(`Material: '${t}' is not a property of THREE.${this.type}.`);continue}s&&s.isColor?s.set(i):s&&s.isVector2&&i&&i.isVector2||s&&s.isEuler&&i&&i.isEuler||s&&s.isVector3&&i&&i.isVector3?s.copy(i):this[t]=i}}toJSON(e){let t=e===void 0||typeof e=="string";t&&(e={textures:{},images:{}});let i={metadata:{version:4.7,type:"Material",generator:"Material.toJSON"}};i.uuid=this.uuid,i.type=this.type,i.blending=this.blending,i.side=this.side,i.shadowSide=this.shadowSide,i.vertexColors=this.vertexColors,i.opacity=this.opacity,i.transparent=this.transparent,i.blendSrc=this.blendSrc,i.blendDst=this.blendDst,i.blendEquation=this.blendEquation,i.blendSrcAlpha=this.blendSrcAlpha,i.blendDstAlpha=this.blendDstAlpha,i.blendEquationAlpha=this.blendEquationAlpha,i.blendColor=this.blendColor.getHex(),i.blendAlpha=this.blendAlpha,i.depthFunc=this.depthFunc,i.depthTest=this.depthTest,i.depthWrite=this.depthWrite,i.colorWrite=this.colorWrite,i.clipIntersection=this.clipIntersection,i.clipShadows=this.clipShadows,i.stencilWriteMask=this.stencilWriteMask,i.stencilFunc=this.stencilFunc,i.stencilRef=this.stencilRef,i.stencilFuncMask=this.stencilFuncMask,i.stencilFail=this.stencilFail,i.stencilZFail=this.stencilZFail,i.stencilZPass=this.stencilZPass,i.stencilWrite=this.stencilWrite,i.polygonOffset=this.polygonOffset,i.polygonOffsetFactor=this.polygonOffsetFactor,i.polygonOffsetUnits=this.polygonOffsetUnits,i.dithering=this.dithering,i.alphaTest=this.alphaTest,i.alphaHash=this.alphaHash,i.alphaToCoverage=this.alphaToCoverage,i.premultipliedAlpha=this.premultipliedAlpha,i.forceSinglePass=this.forceSinglePass,i.allowOverride=this.allowOverride,i.visible=this.visible,i.toneMapped=this.toneMapped,i.name=this.name,this.color&&this.color.isColor&&(i.color=this.color.getHex()),this.roughness!==void 0&&(i.roughness=this.roughness),this.metalness!==void 0&&(i.metalness=this.metalness),this.sheen!==void 0&&(i.sheen=this.sheen),this.sheenColor&&this.sheenColor.isColor&&(i.sheenColor=this.sheenColor.getHex()),this.sheenRoughness!==void 0&&(i.sheenRoughness=this.sheenRoughness),this.emissive&&this.emissive.isColor&&(i.emissive=this.emissive.getHex()),this.emissiveIntensity!==void 0&&(i.emissiveIntensity=this.emissiveIntensity),this.specular&&this.specular.isColor&&(i.specular=this.specular.getHex()),this.specularIntensity!==void 0&&(i.specularIntensity=this.specularIntensity),this.specularColor&&this.specularColor.isColor&&(i.specularColor=this.specularColor.getHex()),this.shininess!==void 0&&(i.shininess=this.shininess),this.clearcoat!==void 0&&(i.clearcoat=this.clearcoat),this.clearcoatRoughness!==void 0&&(i.clearcoatRoughness=this.clearcoatRoughness),this.clearcoatMap&&this.clearcoatMap.isTexture&&(i.clearcoatMap=this.clearcoatMap.toJSON(e).uuid),this.clearcoatRoughnessMap&&this.clearcoatRoughnessMap.isTexture&&(i.clearcoatRoughnessMap=this.clearcoatRoughnessMap.toJSON(e).uuid),this.clearcoatNormalMap&&this.clearcoatNormalMap.isTexture&&(i.clearcoatNormalMap=this.clearcoatNormalMap.toJSON(e).uuid,i.clearcoatNormalScale=this.clearcoatNormalScale.toArray()),this.sheenColorMap&&this.sheenColorMap.isTexture&&(i.sheenColorMap=this.sheenColorMap.toJSON(e).uuid),this.sheenRoughnessMap&&this.sheenRoughnessMap.isTexture&&(i.sheenRoughnessMap=this.sheenRoughnessMap.toJSON(e).uuid),this.dispersion!==void 0&&(i.dispersion=this.dispersion),this.retroreflectivity!==void 0&&(i.retroreflectivity=this.retroreflectivity),this.iridescence!==void 0&&(i.iridescence=this.iridescence),this.iridescenceIOR!==void 0&&(i.iridescenceIOR=this.iridescenceIOR),this.iridescenceThicknessRange!==void 0&&(i.iridescenceThicknessRange=this.iridescenceThicknessRange),this.iridescenceMap&&this.iridescenceMap.isTexture&&(i.iridescenceMap=this.iridescenceMap.toJSON(e).uuid),this.iridescenceThicknessMap&&this.iridescenceThicknessMap.isTexture&&(i.iridescenceThicknessMap=this.iridescenceThicknessMap.toJSON(e).uuid),this.anisotropy!==void 0&&(i.anisotropy=this.anisotropy),this.anisotropyRotation!==void 0&&(i.anisotropyRotation=this.anisotropyRotation),this.anisotropyMap&&this.anisotropyMap.isTexture&&(i.anisotropyMap=this.anisotropyMap.toJSON(e).uuid),this.map&&this.map.isTexture&&(i.map=this.map.toJSON(e).uuid),this.matcap&&this.matcap.isTexture&&(i.matcap=this.matcap.toJSON(e).uuid),this.alphaMap&&this.alphaMap.isTexture&&(i.alphaMap=this.alphaMap.toJSON(e).uuid),this.lightMap&&this.lightMap.isTexture&&(i.lightMap=this.lightMap.toJSON(e).uuid,i.lightMapIntensity=this.lightMapIntensity),this.aoMap&&this.aoMap.isTexture&&(i.aoMap=this.aoMap.toJSON(e).uuid,i.aoMapIntensity=this.aoMapIntensity),this.bumpMap&&this.bumpMap.isTexture&&(i.bumpMap=this.bumpMap.toJSON(e).uuid,i.bumpScale=this.bumpScale),this.normalMap&&this.normalMap.isTexture&&(i.normalMap=this.normalMap.toJSON(e).uuid,i.normalMapType=this.normalMapType,i.normalScale=this.normalScale.toArray()),this.displacementMap&&this.displacementMap.isTexture&&(i.displacementMap=this.displacementMap.toJSON(e).uuid,i.displacementScale=this.displacementScale,i.displacementBias=this.displacementBias),this.roughnessMap&&this.roughnessMap.isTexture&&(i.roughnessMap=this.roughnessMap.toJSON(e).uuid),this.metalnessMap&&this.metalnessMap.isTexture&&(i.metalnessMap=this.metalnessMap.toJSON(e).uuid),this.emissiveMap&&this.emissiveMap.isTexture&&(i.emissiveMap=this.emissiveMap.toJSON(e).uuid),this.specularMap&&this.specularMap.isTexture&&(i.specularMap=this.specularMap.toJSON(e).uuid),this.specularIntensityMap&&this.specularIntensityMap.isTexture&&(i.specularIntensityMap=this.specularIntensityMap.toJSON(e).uuid),this.specularColorMap&&this.specularColorMap.isTexture&&(i.specularColorMap=this.specularColorMap.toJSON(e).uuid),this.envMap&&this.envMap.isTexture&&(i.envMap=this.envMap.toJSON(e).uuid,this.combine!==void 0&&(i.combine=this.combine)),this.envMapRotation!==void 0&&(i.envMapRotation=this.envMapRotation.toArray()),this.envMapIntensity!==void 0&&(i.envMapIntensity=this.envMapIntensity),this.reflectivity!==void 0&&(i.reflectivity=this.reflectivity),this.refractionRatio!==void 0&&(i.refractionRatio=this.refractionRatio),this.gradientMap&&this.gradientMap.isTexture&&(i.gradientMap=this.gradientMap.toJSON(e).uuid),this.transmission!==void 0&&(i.transmission=this.transmission),this.transmissionMap&&this.transmissionMap.isTexture&&(i.transmissionMap=this.transmissionMap.toJSON(e).uuid),this.thickness!==void 0&&(i.thickness=this.thickness),this.thicknessMap&&this.thicknessMap.isTexture&&(i.thicknessMap=this.thicknessMap.toJSON(e).uuid),this.attenuationDistance!==void 0&&(i.attenuationDistance=this.attenuationDistance),this.attenuationColor!==void 0&&(i.attenuationColor=this.attenuationColor.getHex()),this.size!==void 0&&(i.size=this.size),this.sizeAttenuation!==void 0&&(i.sizeAttenuation=this.sizeAttenuation),Array.isArray(this.clippingPlanes)&&this.clippingPlanes.length>0&&(i.clippingPlanes=this.clippingPlanes.map(r=>r.toJSON())),this.rotation!==void 0&&(i.rotation=this.rotation),this.depthPacking!==void 0&&(i.depthPacking=this.depthPacking),this.linewidth!==void 0&&(i.linewidth=this.linewidth),this.linecap!==void 0&&(i.linecap=this.linecap),this.linejoin!==void 0&&(i.linejoin=this.linejoin),this.dashSize!==void 0&&(i.dashSize=this.dashSize),this.gapSize!==void 0&&(i.gapSize=this.gapSize),this.scale!==void 0&&(i.scale=this.scale),this.wireframe!==void 0&&(i.wireframe=this.wireframe),this.wireframeLinewidth!==void 0&&(i.wireframeLinewidth=this.wireframeLinewidth),this.wireframeLinecap!==void 0&&(i.wireframeLinecap=this.wireframeLinecap),this.wireframeLinejoin!==void 0&&(i.wireframeLinejoin=this.wireframeLinejoin),this.flatShading!==void 0&&(i.flatShading=this.flatShading),this.fog!==void 0&&(i.fog=this.fog),Object.keys(this.userData).length>0&&(i.userData=this.userData);function s(r){let o=[];for(let a in r){let l=r[a];delete l.metadata,o.push(l)}return o}if(t){let r=s(e.textures),o=s(e.images);r.length>0&&(i.textures=r),o.length>0&&(i.images=o)}return i}fromJSON(e,t){if(e.uuid!==void 0&&(this.uuid=e.uuid),e.name!==void 0&&(this.name=e.name),e.color!==void 0&&this.color!==void 0&&this.color.setHex(e.color),e.roughness!==void 0&&(this.roughness=e.roughness),e.metalness!==void 0&&(this.metalness=e.metalness),e.sheen!==void 0&&(this.sheen=e.sheen),e.sheenColor!==void 0&&(this.sheenColor=new He().setHex(e.sheenColor)),e.sheenRoughness!==void 0&&(this.sheenRoughness=e.sheenRoughness),e.emissive!==void 0&&this.emissive!==void 0&&this.emissive.setHex(e.emissive),e.specular!==void 0&&this.specular!==void 0&&this.specular.setHex(e.specular),e.specularIntensity!==void 0&&(this.specularIntensity=e.specularIntensity),e.specularColor!==void 0&&this.specularColor!==void 0&&this.specularColor.setHex(e.specularColor),e.shininess!==void 0&&(this.shininess=e.shininess),e.clearcoat!==void 0&&(this.clearcoat=e.clearcoat),e.clearcoatRoughness!==void 0&&(this.clearcoatRoughness=e.clearcoatRoughness),e.dispersion!==void 0&&(this.dispersion=e.dispersion),e.retroreflectivity!==void 0&&(this.retroreflectivity=e.retroreflectivity),e.iridescence!==void 0&&(this.iridescence=e.iridescence),e.iridescenceIOR!==void 0&&(this.iridescenceIOR=e.iridescenceIOR),e.iridescenceThicknessRange!==void 0&&(this.iridescenceThicknessRange=e.iridescenceThicknessRange),e.transmission!==void 0&&(this.transmission=e.transmission),e.thickness!==void 0&&(this.thickness=e.thickness),e.attenuationDistance!==void 0&&(this.attenuationDistance=e.attenuationDistance),e.attenuationColor!==void 0&&this.attenuationColor!==void 0&&this.attenuationColor.setHex(e.attenuationColor),e.anisotropy!==void 0&&(this.anisotropy=e.anisotropy),e.anisotropyRotation!==void 0&&(this.anisotropyRotation=e.anisotropyRotation),e.fog!==void 0&&(this.fog=e.fog),e.flatShading!==void 0&&(this.flatShading=e.flatShading),e.blending!==void 0&&(this.blending=e.blending),e.combine!==void 0&&(this.combine=e.combine),e.side!==void 0&&(this.side=e.side),e.shadowSide!==void 0&&(this.shadowSide=e.shadowSide),e.opacity!==void 0&&(this.opacity=e.opacity),e.transparent!==void 0&&(this.transparent=e.transparent),e.alphaTest!==void 0&&(this.alphaTest=e.alphaTest),e.alphaHash!==void 0&&(this.alphaHash=e.alphaHash),e.depthFunc!==void 0&&(this.depthFunc=e.depthFunc),e.depthTest!==void 0&&(this.depthTest=e.depthTest),e.depthWrite!==void 0&&(this.depthWrite=e.depthWrite),e.colorWrite!==void 0&&(this.colorWrite=e.colorWrite),e.clippingPlanes!==void 0&&(this.clippingPlanes=e.clippingPlanes.map(i=>new Rn().fromJSON(i))),e.clipIntersection!==void 0&&(this.clipIntersection=e.clipIntersection),e.clipShadows!==void 0&&(this.clipShadows=e.clipShadows),e.depthPacking!==void 0&&(this.depthPacking=e.depthPacking),e.blendSrc!==void 0&&(this.blendSrc=e.blendSrc),e.blendDst!==void 0&&(this.blendDst=e.blendDst),e.blendEquation!==void 0&&(this.blendEquation=e.blendEquation),e.blendSrcAlpha!==void 0&&(this.blendSrcAlpha=e.blendSrcAlpha),e.blendDstAlpha!==void 0&&(this.blendDstAlpha=e.blendDstAlpha),e.blendEquationAlpha!==void 0&&(this.blendEquationAlpha=e.blendEquationAlpha),e.blendColor!==void 0&&this.blendColor!==void 0&&this.blendColor.setHex(e.blendColor),e.blendAlpha!==void 0&&(this.blendAlpha=e.blendAlpha),e.stencilWriteMask!==void 0&&(this.stencilWriteMask=e.stencilWriteMask),e.stencilFunc!==void 0&&(this.stencilFunc=e.stencilFunc),e.stencilRef!==void 0&&(this.stencilRef=e.stencilRef),e.stencilFuncMask!==void 0&&(this.stencilFuncMask=e.stencilFuncMask),e.stencilFail!==void 0&&(this.stencilFail=e.stencilFail),e.stencilZFail!==void 0&&(this.stencilZFail=e.stencilZFail),e.stencilZPass!==void 0&&(this.stencilZPass=e.stencilZPass),e.stencilWrite!==void 0&&(this.stencilWrite=e.stencilWrite),e.wireframe!==void 0&&(this.wireframe=e.wireframe),e.wireframeLinewidth!==void 0&&(this.wireframeLinewidth=e.wireframeLinewidth),e.wireframeLinecap!==void 0&&(this.wireframeLinecap=e.wireframeLinecap),e.wireframeLinejoin!==void 0&&(this.wireframeLinejoin=e.wireframeLinejoin),e.rotation!==void 0&&(this.rotation=e.rotation),e.linewidth!==void 0&&(this.linewidth=e.linewidth),e.linecap!==void 0&&(this.linecap=e.linecap),e.linejoin!==void 0&&(this.linejoin=e.linejoin),e.dashSize!==void 0&&(this.dashSize=e.dashSize),e.gapSize!==void 0&&(this.gapSize=e.gapSize),e.scale!==void 0&&(this.scale=e.scale),e.polygonOffset!==void 0&&(this.polygonOffset=e.polygonOffset),e.polygonOffsetFactor!==void 0&&(this.polygonOffsetFactor=e.polygonOffsetFactor),e.polygonOffsetUnits!==void 0&&(this.polygonOffsetUnits=e.polygonOffsetUnits),e.dithering!==void 0&&(this.dithering=e.dithering),e.alphaToCoverage!==void 0&&(this.alphaToCoverage=e.alphaToCoverage),e.premultipliedAlpha!==void 0&&(this.premultipliedAlpha=e.premultipliedAlpha),e.forceSinglePass!==void 0&&(this.forceSinglePass=e.forceSinglePass),e.allowOverride!==void 0&&(this.allowOverride=e.allowOverride),e.visible!==void 0&&(this.visible=e.visible),e.toneMapped!==void 0&&(this.toneMapped=e.toneMapped),e.userData!==void 0&&(this.userData=e.userData),e.vertexColors!==void 0&&(typeof e.vertexColors=="number"?this.vertexColors=e.vertexColors>0:this.vertexColors=e.vertexColors),e.size!==void 0&&(this.size=e.size),e.sizeAttenuation!==void 0&&(this.sizeAttenuation=e.sizeAttenuation),e.map!==void 0&&(this.map=t[e.map]||null),e.matcap!==void 0&&(this.matcap=t[e.matcap]||null),e.alphaMap!==void 0&&(this.alphaMap=t[e.alphaMap]||null),e.bumpMap!==void 0&&(this.bumpMap=t[e.bumpMap]||null),e.bumpScale!==void 0&&(this.bumpScale=e.bumpScale),e.normalMap!==void 0&&(this.normalMap=t[e.normalMap]||null),e.normalMapType!==void 0&&(this.normalMapType=e.normalMapType),e.normalScale!==void 0){let i=e.normalScale;Array.isArray(i)===!1&&(i=[i,i]),this.normalScale=new ze().fromArray(i)}return e.displacementMap!==void 0&&(this.displacementMap=t[e.displacementMap]||null),e.displacementScale!==void 0&&(this.displacementScale=e.displacementScale),e.displacementBias!==void 0&&(this.displacementBias=e.displacementBias),e.roughnessMap!==void 0&&(this.roughnessMap=t[e.roughnessMap]||null),e.metalnessMap!==void 0&&(this.metalnessMap=t[e.metalnessMap]||null),e.emissiveMap!==void 0&&(this.emissiveMap=t[e.emissiveMap]||null),e.emissiveIntensity!==void 0&&(this.emissiveIntensity=e.emissiveIntensity),e.specularMap!==void 0&&(this.specularMap=t[e.specularMap]||null),e.specularIntensityMap!==void 0&&(this.specularIntensityMap=t[e.specularIntensityMap]||null),e.specularColorMap!==void 0&&(this.specularColorMap=t[e.specularColorMap]||null),e.envMap!==void 0&&(this.envMap=t[e.envMap]||null),e.envMapRotation!==void 0&&this.envMapRotation.fromArray(e.envMapRotation),e.envMapIntensity!==void 0&&(this.envMapIntensity=e.envMapIntensity),e.reflectivity!==void 0&&(this.reflectivity=e.reflectivity),e.refractionRatio!==void 0&&(this.refractionRatio=e.refractionRatio),e.lightMap!==void 0&&(this.lightMap=t[e.lightMap]||null),e.lightMapIntensity!==void 0&&(this.lightMapIntensity=e.lightMapIntensity),e.aoMap!==void 0&&(this.aoMap=t[e.aoMap]||null),e.aoMapIntensity!==void 0&&(this.aoMapIntensity=e.aoMapIntensity),e.gradientMap!==void 0&&(this.gradientMap=t[e.gradientMap]||null),e.clearcoatMap!==void 0&&(this.clearcoatMap=t[e.clearcoatMap]||null),e.clearcoatRoughnessMap!==void 0&&(this.clearcoatRoughnessMap=t[e.clearcoatRoughnessMap]||null),e.clearcoatNormalMap!==void 0&&(this.clearcoatNormalMap=t[e.clearcoatNormalMap]||null),e.clearcoatNormalScale!==void 0&&(this.clearcoatNormalScale=new ze().fromArray(e.clearcoatNormalScale)),e.iridescenceMap!==void 0&&(this.iridescenceMap=t[e.iridescenceMap]||null),e.iridescenceThicknessMap!==void 0&&(this.iridescenceThicknessMap=t[e.iridescenceThicknessMap]||null),e.transmissionMap!==void 0&&(this.transmissionMap=t[e.transmissionMap]||null),e.thicknessMap!==void 0&&(this.thicknessMap=t[e.thicknessMap]||null),e.anisotropyMap!==void 0&&(this.anisotropyMap=t[e.anisotropyMap]||null),e.sheenColorMap!==void 0&&(this.sheenColorMap=t[e.sheenColorMap]||null),e.sheenRoughnessMap!==void 0&&(this.sheenRoughnessMap=t[e.sheenRoughnessMap]||null),this}clone(){return new this.constructor().copy(this)}copy(e){this.name=e.name,this.blending=e.blending,this.side=e.side,this.vertexColors=e.vertexColors,this.opacity=e.opacity,this.transparent=e.transparent,this.blendSrc=e.blendSrc,this.blendDst=e.blendDst,this.blendEquation=e.blendEquation,this.blendSrcAlpha=e.blendSrcAlpha,this.blendDstAlpha=e.blendDstAlpha,this.blendEquationAlpha=e.blendEquationAlpha,this.blendColor.copy(e.blendColor),this.blendAlpha=e.blendAlpha,this.depthFunc=e.depthFunc,this.depthTest=e.depthTest,this.depthWrite=e.depthWrite,this.stencilWriteMask=e.stencilWriteMask,this.stencilFunc=e.stencilFunc,this.stencilRef=e.stencilRef,this.stencilFuncMask=e.stencilFuncMask,this.stencilFail=e.stencilFail,this.stencilZFail=e.stencilZFail,this.stencilZPass=e.stencilZPass,this.stencilWrite=e.stencilWrite;let t=e.clippingPlanes,i=null;if(t!==null){let s=t.length;i=new Array(s);for(let r=0;r!==s;++r)i[r]=t[r].clone()}return this.clippingPlanes=i,this.clipIntersection=e.clipIntersection,this.clipShadows=e.clipShadows,this.shadowSide=e.shadowSide,this.colorWrite=e.colorWrite,this.precision=e.precision,this.polygonOffset=e.polygonOffset,this.polygonOffsetFactor=e.polygonOffsetFactor,this.polygonOffsetUnits=e.polygonOffsetUnits,this.dithering=e.dithering,this.alphaTest=e.alphaTest,this.alphaHash=e.alphaHash,this.alphaToCoverage=e.alphaToCoverage,this.premultipliedAlpha=e.premultipliedAlpha,this.forceSinglePass=e.forceSinglePass,this.allowOverride=e.allowOverride,this.visible=e.visible,this.toneMapped=e.toneMapped,this.userData=JSON.parse(JSON.stringify(e.userData)),this}dispose(){this.dispatchEvent({type:"dispose"})}set needsUpdate(e){e===!0&&this.version++}};var Mi=new L,zu=new L,rl=new L,ol=new L,yr=class{constructor(e=new L,t=new L(0,0,-1)){this.origin=e,this.direction=t}set(e,t){return this.origin.copy(e),this.direction.copy(t),this}copy(e){return this.origin.copy(e.origin),this.direction.copy(e.direction),this}at(e,t){return t.copy(this.origin).addScaledVector(this.direction,e)}lookAt(e){return this.direction.copy(e).sub(this.origin).normalize(),this}recast(e){return this.origin.copy(this.at(e,Mi)),this}closestPointToPoint(e,t){t.subVectors(e,this.origin);let i=t.dot(this.direction);return i<0?t.copy(this.origin):t.copy(this.origin).addScaledVector(this.direction,i)}distanceToPoint(e){return Math.sqrt(this.distanceSqToPoint(e))}distanceSqToPoint(e){let t=Mi.subVectors(e,this.origin).dot(this.direction);return t<0?this.origin.distanceToSquared(e):(Mi.copy(this.origin).addScaledVector(this.direction,t),Mi.distanceToSquared(e))}distanceSqToSegment(e,t,i,s){zu.copy(e).add(t).multiplyScalar(.5),rl.copy(t).sub(e).normalize(),ol.copy(this.origin).sub(zu);let r=e.distanceTo(t)*.5,o=-this.direction.dot(rl),a=ol.dot(this.direction),l=-ol.dot(rl),c=ol.lengthSq(),u=Math.abs(1-o*o),d,h,f,g;if(u>0)if(d=o*l-a,h=o*a-l,g=r*u,d>=0)if(h>=-g)if(h<=g){let y=1/u;d*=y,h*=y,f=d*(d+o*h+2*a)+h*(o*d+h+2*l)+c}else h=r,d=Math.max(0,-(o*h+a)),f=-d*d+h*(h+2*l)+c;else h=-r,d=Math.max(0,-(o*h+a)),f=-d*d+h*(h+2*l)+c;else h<=-g?(d=Math.max(0,-(-o*r+a)),h=d>0?-r:Math.min(Math.max(-r,-l),r),f=-d*d+h*(h+2*l)+c):h<=g?(d=0,h=Math.min(Math.max(-r,-l),r),f=h*(h+2*l)+c):(d=Math.max(0,-(o*r+a)),h=d>0?r:Math.min(Math.max(-r,-l),r),f=-d*d+h*(h+2*l)+c);else h=o>0?-r:r,d=Math.max(0,-(o*h+a)),f=-d*d+h*(h+2*l)+c;return i&&i.copy(this.origin).addScaledVector(this.direction,d),s&&s.copy(zu).addScaledVector(rl,h),f}intersectSphere(e,t){if(e.radius<0)return null;Mi.subVectors(e.center,this.origin);let i=Mi.dot(this.direction),s=Mi.dot(Mi)-i*i,r=e.radius*e.radius;if(s>r)return null;let o=Math.sqrt(r-s),a=i-o,l=i+o;return l<0?null:a<0?this.at(l,t):this.at(a,t)}intersectsSphere(e){return e.radius<0?!1:this.distanceSqToPoint(e.center)<=e.radius*e.radius}distanceToPlane(e){let t=e.normal.dot(this.direction);if(t===0)return e.distanceToPoint(this.origin)===0?0:null;let i=-(this.origin.dot(e.normal)+e.constant)/t;return i>=0?i:null}intersectPlane(e,t){let i=this.distanceToPlane(e);return i===null?null:this.at(i,t)}intersectsPlane(e){let t=e.distanceToPoint(this.origin);return t===0||e.normal.dot(this.direction)*t<0}intersectBox(e,t){let i,s,r,o,a,l,c=1/this.direction.x,u=1/this.direction.y,d=1/this.direction.z,h=this.origin;return c>=0?(i=(e.min.x-h.x)*c,s=(e.max.x-h.x)*c):(i=(e.max.x-h.x)*c,s=(e.min.x-h.x)*c),u>=0?(r=(e.min.y-h.y)*u,o=(e.max.y-h.y)*u):(r=(e.max.y-h.y)*u,o=(e.min.y-h.y)*u),i>o||r>s||((r>i||isNaN(i))&&(i=r),(o<s||isNaN(s))&&(s=o),d>=0?(a=(e.min.z-h.z)*d,l=(e.max.z-h.z)*d):(a=(e.max.z-h.z)*d,l=(e.min.z-h.z)*d),i>l||a>s)||((a>i||i!==i)&&(i=a),(l<s||s!==s)&&(s=l),s<0)?null:this.at(i>=0?i:s,t)}intersectsBox(e){return this.intersectBox(e,Mi)!==null}intersectTriangle(e,t,i,s,r){let o=this.origin,a=this.direction,l=a.x,c=a.y,u=a.z,d=e.x-o.x,h=e.y-o.y,f=e.z-o.z,g=t.x-o.x,y=t.y-o.y,m=t.z-o.z,p=i.x-o.x,E=i.y-o.y,w=i.z-o.z,S=Math.abs(l),v=Math.abs(c),A=Math.abs(u),R,b,M,B,D,k,H,F,O,q,X,re;if(S>=v&&S>=A?(M=l,k=d,O=g,re=p,l>=0?(R=c,b=u,B=h,D=f,H=y,F=m,q=E,X=w):(R=u,b=c,B=f,D=h,H=m,F=y,q=w,X=E)):v>=A?(M=c,k=h,O=y,re=E,c>=0?(R=u,b=l,B=f,D=d,H=m,F=g,q=w,X=p):(R=l,b=u,B=d,D=f,H=g,F=m,q=p,X=w)):(M=u,k=f,O=m,re=w,u>=0?(R=l,b=c,B=d,D=h,H=g,F=y,q=p,X=E):(R=c,b=l,B=h,D=d,H=y,F=g,q=E,X=p)),M===0)return null;let K=R/M,ne=b/M,se=1/M,Ue=B-K*k,Fe=D-ne*k,yt=H-K*O,je=F-ne*O,rt=q-K*re,$=X-ne*re,te=rt*je-$*yt,Te=Ue*$-Fe*rt,Oe=yt*Fe-je*Ue;if(s){if(te<0||Te<0||Oe<0)return null}else if((te<0||Te<0||Oe<0)&&(te>0||Te>0||Oe>0))return null;let ve=te+Te+Oe;if(ve===0)return null;let Ke=se*(te*k+Te*O+Oe*re);return(ve>0?Ke<0:Ke>0)?null:this.at(Ke/ve,r)}applyMatrix4(e){return this.origin.applyMatrix4(e),this.direction.transformDirection(e),this}equals(e){return e.origin.equals(this.origin)&&e.direction.equals(this.direction)}clone(){return new this.constructor().copy(this)}},_r=class extends ui{constructor(e){super(),this.isMeshBasicMaterial=!0,this.type="MeshBasicMaterial",this.color=new He(16777215),this.map=null,this.lightMap=null,this.lightMapIntensity=1,this.aoMap=null,this.aoMapIntensity=1,this.specularMap=null,this.alphaMap=null,this.envMap=null,this.envMapRotation=new Ii,this.combine=$l,this.reflectivity=1,this.refractionRatio=.98,this.wireframe=!1,this.wireframeLinewidth=1,this.wireframeLinecap="round",this.wireframeLinejoin="round",this.fog=!0,this.setValues(e)}copy(e){return super.copy(e),this.color.copy(e.color),this.map=e.map,this.lightMap=e.lightMap,this.lightMapIntensity=e.lightMapIntensity,this.aoMap=e.aoMap,this.aoMapIntensity=e.aoMapIntensity,this.specularMap=e.specularMap,this.alphaMap=e.alphaMap,this.envMap=e.envMap,this.envMapRotation.copy(e.envMapRotation),this.combine=e.combine,this.reflectivity=e.reflectivity,this.refractionRatio=e.refractionRatio,this.wireframe=e.wireframe,this.wireframeLinewidth=e.wireframeLinewidth,this.wireframeLinecap=e.wireframeLinecap,this.wireframeLinejoin=e.wireframeLinejoin,this.fog=e.fog,this}},Dp=new ct,ls=new yr,al=new Kn,Pp=new L,ll=new L,cl=new L,ul=new L,Wu=new L,hl=new L,Gp=new L,dl=new L,fn=class extends Jt{constructor(e=new dn,t=new _r){super(),this.isMesh=!0,this.type="Mesh",this.geometry=e,this.material=t,this.morphTargetDictionary=void 0,this.morphTargetInfluences=void 0,this.count=1,this.updateMorphTargets()}copy(e,t){return super.copy(e,t),e.morphTargetInfluences!==void 0&&(this.morphTargetInfluences=e.morphTargetInfluences.slice()),e.morphTargetDictionary!==void 0&&(this.morphTargetDictionary=Object.assign({},e.morphTargetDictionary)),this.material=Array.isArray(e.material)?e.material.slice():e.material,this.geometry=e.geometry,this}updateMorphTargets(){let t=this.geometry.morphAttributes,i=Object.keys(t);if(i.length>0){let s=t[i[0]];if(s!==void 0){this.morphTargetInfluences=[],this.morphTargetDictionary={};for(let r=0,o=s.length;r<o;r++){let a=s[r].name||String(r);this.morphTargetInfluences.push(0),this.morphTargetDictionary[a]=r}}}}getVertexPosition(e,t){let i=this.geometry,s=i.attributes.position,r=i.morphAttributes.position,o=i.morphTargetsRelative;t.fromBufferAttribute(s,e);let a=this.morphTargetInfluences;if(r&&a){hl.set(0,0,0);for(let l=0,c=r.length;l<c;l++){let u=a[l],d=r[l];u!==0&&(Wu.fromBufferAttribute(d,e),o?hl.addScaledVector(Wu,u):hl.addScaledVector(Wu.sub(t),u))}t.add(hl)}return t}intersectsFrustum(e){return e.intersectsObject(this)}raycast(e,t){let i=this.geometry,s=this.material,r=this.matrixWorld;s!==void 0&&(i.boundingSphere===null&&i.computeBoundingSphere(),al.copy(i.boundingSphere),al.applyMatrix4(r),ls.copy(e.ray).recast(e.near),!(al.containsPoint(ls.origin)===!1&&(ls.intersectSphere(al,Pp)===null||ls.origin.distanceToSquared(Pp)>(e.far-e.near)**2))&&(Dp.copy(r).invert(),ls.copy(e.ray).applyMatrix4(Dp),!(i.boundingBox!==null&&ls.intersectsBox(i.boundingBox)===!1)&&this._computeIntersections(e,t,ls)))}_computeIntersections(e,t,i){let s,r=this.geometry,o=this.material,a=r.index,l=r.attributes.position,c=r.attributes.uv,u=r.attributes.uv1,d=r.attributes.normal,h=r.groups,f=r.drawRange;if(a!==null)if(Array.isArray(o))for(let g=0,y=h.length;g<y;g++){let m=h[g],p=o[m.materialIndex],E=Math.max(m.start,f.start),w=Math.min(a.count,Math.min(m.start+m.count,f.start+f.count));for(let S=E,v=w;S<v;S+=3){let A=a.getX(S),R=a.getX(S+1),b=a.getX(S+2);s=fl(this,p,e,i,c,u,d,A,R,b),s&&(s.faceIndex=Math.floor(S/3),s.face.materialIndex=m.materialIndex,t.push(s))}}else{let g=Math.max(0,f.start),y=Math.min(a.count,f.start+f.count);for(let m=g,p=y;m<p;m+=3){let E=a.getX(m),w=a.getX(m+1),S=a.getX(m+2);s=fl(this,o,e,i,c,u,d,E,w,S),s&&(s.faceIndex=Math.floor(m/3),t.push(s))}}else if(l!==void 0)if(Array.isArray(o))for(let g=0,y=h.length;g<y;g++){let m=h[g],p=o[m.materialIndex],E=Math.max(m.start,f.start),w=Math.min(l.count,Math.min(m.start+m.count,f.start+f.count));for(let S=E,v=w;S<v;S+=3){let A=S,R=S+1,b=S+2;s=fl(this,p,e,i,c,u,d,A,R,b),s&&(s.faceIndex=Math.floor(S/3),s.face.materialIndex=m.materialIndex,t.push(s))}}else{let g=Math.max(0,f.start),y=Math.min(l.count,f.start+f.count);for(let m=g,p=y;m<p;m+=3){let E=m,w=m+1,S=m+2;s=fl(this,o,e,i,c,u,d,E,w,S),s&&(s.faceIndex=Math.floor(m/3),t.push(s))}}}};function cy(n,e,t,i,s,r,o,a){let l;if(e.side===Dt?l=i.intersectTriangle(o,r,s,!0,a):l=i.intersectTriangle(s,r,o,e.side===Jn,a),l===null)return null;dl.copy(a),dl.applyMatrix4(n.matrixWorld);let c=t.ray.origin.distanceTo(dl);return c<t.near||c>t.far?null:{distance:c,point:dl.clone(),object:n}}function fl(n,e,t,i,s,r,o,a,l,c){n.getVertexPosition(a,ll),n.getVertexPosition(l,cl),n.getVertexPosition(c,ul);let u=cy(n,e,t,i,ll,cl,ul,Gp);if(u){let d=new L;Ri.getBarycoord(Gp,ll,cl,ul,d),s&&(u.uv=Ri.getInterpolatedAttribute(s,a,l,c,d,new ze)),r&&(u.uv1=Ri.getInterpolatedAttribute(r,a,l,c,d,new ze)),o&&(u.normal=Ri.getInterpolatedAttribute(o,a,l,c,d,new L),u.normal.dot(i.direction)>0&&u.normal.multiplyScalar(-1));let h={a,b:l,c,normal:new L,materialIndex:0};Ri.getNormal(ll,cl,ul,h.normal),u.face=h,u.barycoord=d}return u}var ps=class extends $t{constructor(e=null,t=1,i=1,s,r,o,a,l,c=Rt,u=Rt,d,h){super(null,o,a,l,c,u,s,r,d,h),this.isDataTexture=!0,this.image={data:e,width:t,height:i},this.generateMipmaps=!1,this.flipY=!1,this.unpackAlignment=1}};var cs=new Kn,uy=new ze(.5,.5),pl=new L,ms=class{constructor(e=new Rn,t=new Rn,i=new Rn,s=new Rn,r=new Rn,o=new Rn){this.planes=[e,t,i,s,r,o]}set(e,t,i,s,r,o){let a=this.planes;return a[0].copy(e),a[1].copy(t),a[2].copy(i),a[3].copy(s),a[4].copy(r),a[5].copy(o),this}copy(e){let t=this.planes;for(let i=0;i<6;i++)t[i].copy(e.planes[i]);return this}setFromProjectionMatrix(e,t=hn,i=!1){let s=this.planes,r=e.elements,o=r[0],a=r[1],l=r[2],c=r[3],u=r[4],d=r[5],h=r[6],f=r[7],g=r[8],y=r[9],m=r[10],p=r[11],E=r[12],w=r[13],S=r[14],v=r[15];if(s[0].setComponents(c-o,f-u,p-g,v-E).normalize(),s[1].setComponents(c+o,f+u,p+g,v+E).normalize(),s[2].setComponents(c+a,f+d,p+y,v+w).normalize(),s[3].setComponents(c-a,f-d,p-y,v-w).normalize(),i)s[4].setComponents(l,h,m,S).normalize(),s[5].setComponents(c-l,f-h,p-m,v-S).normalize();else if(s[4].setComponents(c-l,f-h,p-m,v-S).normalize(),t===hn)s[5].setComponents(c+l,f+h,p+m,v+S).normalize();else if(t===li)s[5].setComponents(l,h,m,S).normalize();else throw new Error("THREE.Frustum.setFromProjectionMatrix(): Invalid coordinate system: "+t);return this}intersectsObject(e){if(e.boundingSphere!==void 0)e.boundingSphere===null&&e.computeBoundingSphere(),cs.copy(e.boundingSphere).applyMatrix4(e.matrixWorld);else{let t=e.geometry;t.boundingSphere===null&&t.computeBoundingSphere(),cs.copy(t.boundingSphere).applyMatrix4(e.matrixWorld)}return this.intersectsSphere(cs)}intersectsSprite(e){cs.center.set(0,0,0);let t=uy.distanceTo(e.center);return cs.radius=.7071067811865476+t,cs.applyMatrix4(e.matrixWorld),this.intersectsSphere(cs)}intersectsSphere(e){let t=this.planes,i=e.center,s=-e.radius;for(let r=0;r<6;r++)if(t[r].distanceToPoint(i)<s)return!1;return!0}intersectsBox(e){let t=this.planes;for(let i=0;i<6;i++){let s=t[i];if(pl.x=s.normal.x>0?e.max.x:e.min.x,pl.y=s.normal.y>0?e.max.y:e.min.y,pl.z=s.normal.z>0?e.max.z:e.min.z,s.distanceToPoint(pl)<0)return!1}return!0}containsPoint(e){let t=this.planes;for(let i=0;i<6;i++)if(t[i].distanceToPoint(e)<0)return!1;return!0}clone(){return new this.constructor().copy(this)}};var gs=class extends ui{constructor(e){super(),this.isLineBasicMaterial=!0,this.type="LineBasicMaterial",this.color=new He(16777215),this.map=null,this.linewidth=1,this.linecap="round",this.linejoin="round",this.fog=!0,this.setValues(e)}copy(e){return super.copy(e),this.color.copy(e.color),this.map=e.map,this.linewidth=e.linewidth,this.linecap=e.linecap,this.linejoin=e.linejoin,this.fog=e.fog,this}},Ml=new L,Rl=new L,Vp=new ct,no=new yr,ml=new Kn,Hu=new L,Op=new L,wl=class extends Jt{constructor(e=new dn,t=new gs){super(),this.isLine=!0,this.type="Line",this.geometry=e,this.material=t,this.morphTargetDictionary=void 0,this.morphTargetInfluences=void 0,this.updateMorphTargets()}copy(e,t){return super.copy(e,t),this.material=Array.isArray(e.material)?e.material.slice():e.material,this.geometry=e.geometry,this}computeLineDistances(){let e=this.geometry;if(e.index===null){let t=e.attributes.position,i=[0];for(let s=1,r=t.count;s<r;s++)Ml.fromBufferAttribute(t,s-1),Rl.fromBufferAttribute(t,s),i[s]=i[s-1],i[s]+=Ml.distanceTo(Rl);e.setAttribute("lineDistance",new nn(i,1))}else pe("Line.computeLineDistances(): Computation only possible with non-indexed BufferGeometry.");return this}intersectsFrustum(e){return e.intersectsObject(this)}raycast(e,t){let i=this.geometry,s=this.matrixWorld,r=e.params.Line.threshold,o=i.drawRange;if(i.boundingSphere===null&&i.computeBoundingSphere(),ml.copy(i.boundingSphere),ml.applyMatrix4(s),ml.radius+=r,e.ray.intersectsSphere(ml)===!1)return;Vp.copy(s).invert(),no.copy(e.ray).applyMatrix4(Vp);let a=r/((this.scale.x+this.scale.y+this.scale.z)/3),l=a*a,c=this.isLineSegments?2:1,u=i.index,h=i.attributes.position;if(u!==null){let f=Math.max(0,o.start),g=Math.min(u.count,o.start+o.count);for(let y=f,m=g-1;y<m;y+=c){let p=u.getX(y),E=u.getX(y+1),w=gl(this,e,no,l,p,E,y);w&&t.push(w)}if(this.isLineLoop){let y=u.getX(g-1),m=u.getX(f),p=gl(this,e,no,l,y,m,g-1);p&&t.push(p)}}else{let f=Math.max(0,o.start),g=Math.min(h.count,o.start+o.count);for(let y=f,m=g-1;y<m;y+=c){let p=gl(this,e,no,l,y,y+1,y);p&&t.push(p)}if(this.isLineLoop){let y=gl(this,e,no,l,g-1,f,g-1);y&&t.push(y)}}}updateMorphTargets(){let t=this.geometry.morphAttributes,i=Object.keys(t);if(i.length>0){let s=t[i[0]];if(s!==void 0){this.morphTargetInfluences=[],this.morphTargetDictionary={};for(let r=0,o=s.length;r<o;r++){let a=s[r].name||String(r);this.morphTargetInfluences.push(0),this.morphTargetDictionary[a]=r}}}}};function gl(n,e,t,i,s,r,o){let a=n.geometry.attributes.position;if(Ml.fromBufferAttribute(a,s),Rl.fromBufferAttribute(a,r),t.distanceSqToSegment(Ml,Rl,Hu,Op)>i)return;Hu.applyMatrix4(n.matrixWorld);let c=e.ray.origin.distanceTo(Hu);if(!(c<e.near||c>e.far))return{distance:c,point:Op.clone().applyMatrix4(n.matrixWorld),index:o,face:null,faceIndex:null,barycoord:null,object:n}}var kp=new L,zp=new L,yo=class extends wl{constructor(e,t){super(e,t),this.isLineSegments=!0,this.type="LineSegments"}computeLineDistances(){let e=this.geometry;if(e.index===null){let t=e.attributes.position,i=[];for(let s=0,r=t.count;s<r;s+=2)kp.fromBufferAttribute(t,s),zp.fromBufferAttribute(t,s+1),i[s]=s===0?0:i[s-1],i[s+1]=i[s]+kp.distanceTo(zp);e.setAttribute("lineDistance",new nn(i,1))}else pe("LineSegments.computeLineDistances(): Computation only possible with non-indexed BufferGeometry.");return this}};var _o=class extends $t{constructor(e,t){super({width:e,height:t}),this.isFramebufferTexture=!0,this.magFilter=Rt,this.minFilter=Rt,this.generateMipmaps=!1,this.needsUpdate=!0}};var xs=class extends $t{constructor(e=[],t=hi,i,s,r,o,a,l,c,u){super(e,t,i,s,r,o,a,l,c,u),this.isCubeTexture=!0,this.flipY=!1}get images(){return this.image}set images(e){this.image=e}};var $n=class extends $t{constructor(e,t,i=bn,s,r,o,a=Rt,l=Rt,c,u=Un,d=1){if(u!==Un&&u!==fi)throw new Error("THREE.DepthTexture: format must be either THREE.DepthFormat or THREE.DepthStencilFormat");let h={width:e,height:t,depth:d};super(h,s,r,o,a,l,u,i,c),this.isDepthTexture=!0,this.flipY=!1,this.generateMipmaps=!1,this.compareFunction=null}copy(e){return super.copy(e),this.source=new hr(Object.assign({},e.image)),this.compareFunction=e.compareFunction,this}toJSON(e){let t=super.toJSON(e);return t.compareFunction=this.compareFunction,t}},bo=class extends $n{constructor(e,t=bn,i=hi,s,r,o=Rt,a=Rt,l,c=Un){let u={width:e,height:e,depth:1},d=[u,u,u,u,u,u];super(e,e,t,i,s,r,o,a,l,c),this.image=d,this.isCubeDepthTexture=!0,this.isCubeTexture=!0}get images(){return this.image}set images(e){this.image=e}},So=class extends $t{constructor(e=null){super(),this.sourceTexture=e,this.isExternalTexture=!0}copy(e){return super.copy(e),this.sourceTexture=e.sourceTexture,this}},ys=class n extends dn{constructor(e=1,t=1,i=1,s=1,r=1,o=1){super(),this.type="BoxGeometry",this.parameters={width:e,height:t,depth:i,widthSegments:s,heightSegments:r,depthSegments:o};let a=this;s=Math.floor(s),r=Math.floor(r),o=Math.floor(o);let l=[],c=[],u=[],d=[],h=0,f=0;g("z","y","x",-1,-1,i,t,e,o,r,0),g("z","y","x",1,-1,i,t,-e,o,r,1),g("x","z","y",1,1,e,i,t,s,o,2),g("x","z","y",1,-1,e,i,-t,s,o,3),g("x","y","z",1,-1,e,t,i,s,r,4),g("x","y","z",-1,-1,e,t,-i,s,r,5),this.setIndex(l),this.setAttribute("position",new nn(c,3)),this.setAttribute("normal",new nn(u,3)),this.setAttribute("uv",new nn(d,2));function g(y,m,p,E,w,S,v,A,R,b,M){let B=S/R,D=v/b,k=S/2,H=v/2,F=A/2,O=R+1,q=b+1,X=0,re=0,K=new L;for(let ne=0;ne<q;ne++){let se=ne*D-H;for(let Ue=0;Ue<O;Ue++){let Fe=Ue*B-k;K[y]=Fe*E,K[m]=se*w,K[p]=F,c.push(K.x,K.y,K.z),K[y]=0,K[m]=0,K[p]=A>0?1:-1,u.push(K.x,K.y,K.z),d.push(Ue/R),d.push(1-ne/b),X+=1}}for(let ne=0;ne<b;ne++)for(let se=0;se<R;se++){let Ue=h+se+O*ne,Fe=h+se+O*(ne+1),yt=h+(se+1)+O*(ne+1),je=h+(se+1)+O*ne;l.push(Ue,Fe,je),l.push(Fe,yt,je),re+=6}a.addGroup(f,re,M),f+=re,h+=X}}copy(e){return super.copy(e),this.parameters=Object.assign({},e.parameters),this}static fromJSON(e){return new n(e.width,e.height,e.depth,e.widthSegments,e.heightSegments,e.depthSegments)}};var xl=new L,yl=new L,Yu=new L,_l=new Ri,vo=class extends dn{constructor(e=null,t=1){if(super(),this.type="EdgesGeometry",this.parameters={geometry:e,thresholdAngle:t},e!==null){let s=Math.pow(10,4),r=Math.cos(rr*t),o=e.getIndex(),a=e.getAttribute("position"),l=o?o.count:a.count,c=[0,0,0],u=["a","b","c"],d=new Array(3),h={},f=[];for(let g=0;g<l;g+=3){o?(c[0]=o.getX(g),c[1]=o.getX(g+1),c[2]=o.getX(g+2)):(c[0]=g,c[1]=g+1,c[2]=g+2);let{a:y,b:m,c:p}=_l;if(y.fromBufferAttribute(a,c[0]),m.fromBufferAttribute(a,c[1]),p.fromBufferAttribute(a,c[2]),_l.getNormal(Yu),d[0]=`${Math.round(y.x*s)},${Math.round(y.y*s)},${Math.round(y.z*s)}`,d[1]=`${Math.round(m.x*s)},${Math.round(m.y*s)},${Math.round(m.z*s)}`,d[2]=`${Math.round(p.x*s)},${Math.round(p.y*s)},${Math.round(p.z*s)}`,!(d[0]===d[1]||d[1]===d[2]||d[2]===d[0]))for(let E=0;E<3;E++){let w=(E+1)%3,S=d[E],v=d[w],A=_l[u[E]],R=_l[u[w]],b=`${S}_${v}`,M=`${v}_${S}`;M in h&&h[M]?(Yu.dot(h[M].normal)<=r&&(f.push(A.x,A.y,A.z),f.push(R.x,R.y,R.z)),h[M]=null):b in h||(h[b]={index0:c[E],index1:c[w],normal:Yu.clone()})}}for(let g in h)if(h[g]){let{index0:y,index1:m}=h[g];xl.fromBufferAttribute(a,y),yl.fromBufferAttribute(a,m),f.push(xl.x,xl.y,xl.z),f.push(yl.x,yl.y,yl.z)}this.setAttribute("position",new nn(f,3))}}copy(e){return super.copy(e),this.parameters=Object.assign({},e.parameters),this}};var br=class n extends dn{constructor(e=1,t=1,i=1,s=1){super(),this.type="PlaneGeometry",this.parameters={width:e,height:t,widthSegments:i,heightSegments:s};let r=e/2,o=t/2,a=Math.floor(i),l=Math.floor(s),c=a+1,u=l+1,d=e/a,h=t/l,f=[],g=[],y=[],m=[];for(let p=0;p<u;p++){let E=p*h-o;for(let w=0;w<c;w++){let S=w*d-r;g.push(S,-E,0),y.push(0,0,1),m.push(w/a),m.push(1-p/l)}}for(let p=0;p<l;p++)for(let E=0;E<a;E++){let w=E+c*p,S=E+c*(p+1),v=E+1+c*(p+1),A=E+1+c*p;f.push(w,S,A),f.push(S,v,A)}this.setIndex(f),this.setAttribute("position",new nn(g,3)),this.setAttribute("normal",new nn(y,3)),this.setAttribute("uv",new nn(m,2))}copy(e){return super.copy(e),this.parameters=Object.assign({},e.parameters),this}static fromJSON(e){return new n(e.width,e.height,e.widthSegments,e.heightSegments)}};function Cs(n){let e={};for(let t in n){e[t]={};for(let i in n[t]){let s=n[t][i];if(Wp(s))s.isRenderTargetTexture?(pe("UniformsUtils: Textures of render targets cannot be cloned via cloneUniforms() or mergeUniforms()."),e[t][i]=null):e[t][i]=s.clone();else if(Array.isArray(s))if(Wp(s[0])){let r=[];for(let o=0,a=s.length;o<a;o++)r[o]=s[o].clone();e[t][i]=r}else e[t][i]=s.slice();else e[t][i]=s}}return e}function rn(n){let e={};for(let t=0;t<n.length;t++){let i=Cs(n[t]);for(let s in i)e[s]=i[s]}return e}function Wp(n){return n&&(n.isColor||n.isMatrix3||n.isMatrix4||n.isVector2||n.isVector3||n.isVector4||n.isTexture||n.isQuaternion)}function hy(n){let e=[];for(let t=0;t<n.length;t++)e.push(n[t].clone());return e}function Uh(n){let e=n.getRenderTarget();return e===null?n.outputColorSpace:e.isXRRenderTarget===!0?e.texture.colorSpace:Ve.workingColorSpace}var nm={clone:Cs,merge:rn},dy=`void main() {
	gl_Position = projectionMatrix * modelViewMatrix * vec4( position, 1.0 );
}`,fy=`void main() {
	gl_FragColor = vec4( 1.0, 0.0, 0.0, 1.0 );
}`,Cn=class extends ui{constructor(e){super(),this.isShaderMaterial=!0,this.type="ShaderMaterial",this.defines={},this.uniforms={},this.uniformsGroups=[],this.vertexShader=dy,this.fragmentShader=fy,this.linewidth=1,this.wireframe=!1,this.wireframeLinewidth=1,this.fog=!1,this.lights=!1,this.clipping=!1,this.forceSinglePass=!0,this.extensions={clipCullDistance:!1,multiDraw:!1},this.defaultAttributeValues={color:[1,1,1],uv:[0,0],uv1:[0,0]},this.index0AttributeName=void 0,this.uniformsNeedUpdate=!1,this.glslVersion=null,e!==void 0&&this.setValues(e)}copy(e){return super.copy(e),this.fragmentShader=e.fragmentShader,this.vertexShader=e.vertexShader,this.uniforms=Cs(e.uniforms),this.uniformsGroups=hy(e.uniformsGroups),this.defines=Object.assign({},e.defines),this.wireframe=e.wireframe,this.wireframeLinewidth=e.wireframeLinewidth,this.fog=e.fog,this.lights=e.lights,this.clipping=e.clipping,this.extensions=Object.assign({},e.extensions),this.glslVersion=e.glslVersion,this.defaultAttributeValues=Object.assign({},e.defaultAttributeValues),this.index0AttributeName=e.index0AttributeName,this.uniformsNeedUpdate=e.uniformsNeedUpdate,this}toJSON(e){let t=super.toJSON(e);t.glslVersion=this.glslVersion,t.uniforms={};for(let s in this.uniforms){let o=this.uniforms[s].value;o&&o.isTexture?t.uniforms[s]={type:"t",value:o.toJSON(e).uuid}:o&&o.isColor?t.uniforms[s]={type:"c",value:o.getHex()}:o&&o.isVector2?t.uniforms[s]={type:"v2",value:o.toArray()}:o&&o.isVector3?t.uniforms[s]={type:"v3",value:o.toArray()}:o&&o.isVector4?t.uniforms[s]={type:"v4",value:o.toArray()}:o&&o.isMatrix3?t.uniforms[s]={type:"m3",value:o.toArray()}:o&&o.isMatrix4?t.uniforms[s]={type:"m4",value:o.toArray()}:t.uniforms[s]={value:o}}Object.keys(this.defines).length>0&&(t.defines=this.defines),t.vertexShader=this.vertexShader,t.fragmentShader=this.fragmentShader,t.lights=this.lights,t.clipping=this.clipping;let i={};for(let s in this.extensions)this.extensions[s]===!0&&(i[s]=!0);return Object.keys(i).length>0&&(t.extensions=i),t}fromJSON(e,t){if(super.fromJSON(e,t),e.uniforms!==void 0)for(let i in e.uniforms){let s=e.uniforms[i];switch(this.uniforms[i]={},s.type){case"t":this.uniforms[i].value=t[s.value]||null;break;case"c":this.uniforms[i].value=new He().setHex(s.value);break;case"v2":this.uniforms[i].value=new ze().fromArray(s.value);break;case"v3":this.uniforms[i].value=new L().fromArray(s.value);break;case"v4":this.uniforms[i].value=new dt().fromArray(s.value);break;case"m3":this.uniforms[i].value=new Ge().fromArray(s.value);break;case"m4":this.uniforms[i].value=new ct().fromArray(s.value);break;default:this.uniforms[i].value=s.value}}if(e.defines!==void 0&&(this.defines=e.defines),e.vertexShader!==void 0&&(this.vertexShader=e.vertexShader),e.fragmentShader!==void 0&&(this.fragmentShader=e.fragmentShader),e.glslVersion!==void 0&&(this.glslVersion=e.glslVersion),e.extensions!==void 0)for(let i in e.extensions)this.extensions[i]=e.extensions[i];return e.lights!==void 0&&(this.lights=e.lights),e.clipping!==void 0&&(this.clipping=e.clipping),this}},Cl=class extends Cn{constructor(e){super(e),this.isRawShaderMaterial=!0,this.type="RawShaderMaterial"}},Sr=class extends ui{constructor(e){super(),this.isMeshStandardMaterial=!0,this.type="MeshStandardMaterial",this.defines={STANDARD:""},this.color=new He(16777215),this.roughness=1,this.metalness=0,this.map=null,this.lightMap=null,this.lightMapIntensity=1,this.aoMap=null,this.aoMapIntensity=1,this.emissive=new He(0),this.emissiveIntensity=1,this.emissiveMap=null,this.bumpMap=null,this.bumpScale=1,this.normalMap=null,this.normalMapType=Ki,this.normalScale=new ze(1,1),this.displacementMap=null,this.displacementScale=1,this.displacementBias=0,this.roughnessMap=null,this.metalnessMap=null,this.alphaMap=null,this.envMap=null,this.envMapRotation=new Ii,this.envMapIntensity=1,this.wireframe=!1,this.wireframeLinewidth=1,this.wireframeLinecap="round",this.wireframeLinejoin="round",this.flatShading=!1,this.fog=!0,this.setValues(e)}copy(e){return super.copy(e),this.defines={STANDARD:""},this.color.copy(e.color),this.roughness=e.roughness,this.metalness=e.metalness,this.map=e.map,this.lightMap=e.lightMap,this.lightMapIntensity=e.lightMapIntensity,this.aoMap=e.aoMap,this.aoMapIntensity=e.aoMapIntensity,this.emissive.copy(e.emissive),this.emissiveMap=e.emissiveMap,this.emissiveIntensity=e.emissiveIntensity,this.bumpMap=e.bumpMap,this.bumpScale=e.bumpScale,this.normalMap=e.normalMap,this.normalMapType=e.normalMapType,this.normalScale.copy(e.normalScale),this.displacementMap=e.displacementMap,this.displacementScale=e.displacementScale,this.displacementBias=e.displacementBias,this.roughnessMap=e.roughnessMap,this.metalnessMap=e.metalnessMap,this.alphaMap=e.alphaMap,this.envMap=e.envMap,this.envMapRotation.copy(e.envMapRotation),this.envMapIntensity=e.envMapIntensity,this.wireframe=e.wireframe,this.wireframeLinewidth=e.wireframeLinewidth,this.wireframeLinecap=e.wireframeLinecap,this.wireframeLinejoin=e.wireframeLinejoin,this.flatShading=e.flatShading,this.fog=e.fog,this}};var Nl=class extends ui{constructor(e){super(),this.isMeshDepthMaterial=!0,this.type="MeshDepthMaterial",this.depthPacking=qp,this.map=null,this.alphaMap=null,this.displacementMap=null,this.displacementScale=1,this.displacementBias=0,this.wireframe=!1,this.wireframeLinewidth=1,this.setValues(e)}copy(e){return super.copy(e),this.depthPacking=e.depthPacking,this.map=e.map,this.alphaMap=e.alphaMap,this.displacementMap=e.displacementMap,this.displacementScale=e.displacementScale,this.displacementBias=e.displacementBias,this.wireframe=e.wireframe,this.wireframeLinewidth=e.wireframeLinewidth,this}},Il=class extends ui{constructor(e){super(),this.isMeshDistanceMaterial=!0,this.type="MeshDistanceMaterial",this.map=null,this.alphaMap=null,this.displacementMap=null,this.displacementScale=1,this.displacementBias=0,this.setValues(e)}copy(e){return super.copy(e),this.map=e.map,this.alphaMap=e.alphaMap,this.displacementMap=e.displacementMap,this.displacementScale=e.displacementScale,this.displacementBias=e.displacementBias,this}};function nr(n,e){return!n||n.constructor===e?n:typeof e.BYTES_PER_ELEMENT=="number"?new e(n):Array.prototype.slice.call(n)}function Zu(n){return n!==void 0&&n.inTangents!==void 0&&n.outTangents!==void 0}var Hi=class{constructor(e,t,i,s){this.parameterPositions=e,this._cachedIndex=0,this.resultBuffer=s!==void 0?s:new t.constructor(i),this.sampleValues=t,this.valueSize=i,this.settings=null,this.DefaultSettings_={}}evaluate(e){let t=this.parameterPositions,i=this._cachedIndex,s=t[i],r=t[i-1];n:{e:{let o;t:{i:if(!(e<s)){for(let a=i+2;;){if(s===void 0){if(e<r)break i;return i=t.length,this._cachedIndex=i,this.copySampleValue_(i-1)}if(i===a)break;if(r=s,s=t[++i],e<s)break e}o=t.length;break t}if(!(e>=r)){let a=t[1];e<a&&(i=2,r=a);for(let l=i-2;;){if(r===void 0)return this._cachedIndex=0,this.copySampleValue_(0);if(i===l)break;if(s=r,r=t[--i-1],e>=r)break e}o=i,i=0;break t}break n}for(;i<o;){let a=i+o>>>1;e<t[a]?o=a:i=a+1}if(s=t[i],r=t[i-1],r===void 0)return this._cachedIndex=0,this.copySampleValue_(0);if(s===void 0)return i=t.length,this._cachedIndex=i,this.copySampleValue_(i-1)}this._cachedIndex=i,this.intervalChanged_(i,r,s)}return this.interpolate_(i,r,e,s)}getSettings_(){return this.settings||this.DefaultSettings_}copySampleValue_(e){let t=this.resultBuffer,i=this.sampleValues,s=this.valueSize,r=e*s;for(let o=0;o!==s;++o)t[o]=i[r+o];return t}interpolate_(){throw new Error("THREE.Interpolant: Call to abstract method.")}intervalChanged_(){}},Bl=class extends Hi{constructor(e,t,i,s){super(e,t,i,s),this._weightPrev=-0,this._offsetPrev=-0,this._weightNext=-0,this._offsetNext=-0,this.DefaultSettings_={endingStart:$u,endingEnd:$u}}intervalChanged_(e,t,i){let s=this.parameterPositions,r=e-2,o=e+1,a=s[r],l=s[o];if(a===void 0)switch(this.getSettings_().endingStart){case Ju:r=e,a=2*t-i;break;case qu:r=s.length-2,a=t+s[r]-s[r+1];break;default:r=e,a=i}if(l===void 0)switch(this.getSettings_().endingEnd){case Ju:o=e,l=2*i-t;break;case qu:o=1,l=i+s[1]-s[0];break;default:o=e-1,l=t}let c=(i-t)*.5,u=this.valueSize;this._weightPrev=c/(t-a),this._weightNext=c/(l-i),this._offsetPrev=r*u,this._offsetNext=o*u}interpolate_(e,t,i,s){let r=this.resultBuffer,o=this.sampleValues,a=this.valueSize,l=e*a,c=l-a,u=this._offsetPrev,d=this._offsetNext,h=this._weightPrev,f=this._weightNext,g=(i-t)/(s-t),y=g*g,m=y*g,p=-h*m+2*h*y-h*g,E=(1+h)*m+(-1.5-2*h)*y+(-.5+h)*g+1,w=(-1-f)*m+(1.5+f)*y+.5*g,S=f*m-f*y;for(let v=0;v!==a;++v)r[v]=p*o[u+v]+E*o[c+v]+w*o[l+v]+S*o[d+v];return r}},Fl=class extends Hi{constructor(e,t,i,s){super(e,t,i,s)}interpolate_(e,t,i,s){let r=this.resultBuffer,o=this.sampleValues,a=this.valueSize,l=e*a,c=l-a,u=(i-t)/(s-t),d=1-u;for(let h=0;h!==a;++h)r[h]=o[c+h]*d+o[l+h]*u;return r}},Ll=class extends Hi{constructor(e,t,i,s){super(e,t,i,s)}interpolate_(e){return this.copySampleValue_(e-1)}},Ul=class extends Hi{interpolate_(e,t,i,s){let r=this.resultBuffer,o=this.sampleValues,a=this.valueSize,l=e*a,c=l-a,u=this.inTangents,d=this.outTangents;if(!u||!d){let g=(i-t)/(s-t),y=1-g;for(let m=0;m!==a;++m)r[m]=o[c+m]*y+o[l+m]*g;return r}let h=a*2,f=e-1;for(let g=0;g!==a;++g){let y=o[c+g],m=o[l+g],p=f*h+g*2,E=d[p],w=d[p+1],S=e*h+g*2,v=u[S],A=u[S+1],R=my(i,t,E,v,s);r[g]=im(R,y,w,A,m)}return r}};function im(n,e,t,i,s){let r=1-n;return r*r*r*e+3*r*r*n*t+3*r*n*n*i+n*n*n*s}function py(n,e,t,i,s){let r=1-n;return 3*r*r*(t-e)+6*r*n*(i-t)+3*n*n*(s-i)}function my(n,e,t,i,s){let r=(n-e)/(s-e);for(let o=0;o<8;o++){let a=im(r,e,t,i,s)-n;if(Math.abs(a)<1e-10)break;let l=py(r,e,t,i,s);if(Math.abs(l)<1e-10)break;r=Math.max(0,Math.min(1,r-a/l))}return r}var Nn=class{constructor(e,t,i,s){if(e===void 0)throw new Error("THREE.KeyframeTrack: track name is undefined");if(t===void 0||t.length===0)throw new Error("THREE.KeyframeTrack: no keyframes in track named "+e);this.name=e,this.times=nr(t,this.TimeBufferType),this.values=nr(i,this.ValueBufferType),this.setInterpolation(s||this.DefaultInterpolation)}static toJSON(e){let t=e.constructor,i;if(t.toJSON!==this.toJSON)i=t.toJSON(e);else{i={name:e.name,times:nr(e.times,Array),values:nr(e.values,Array)};let s=e.getInterpolation();s!==e.DefaultInterpolation&&(i.interpolation=s),Zu(e.settings)&&(i.settings={inTangents:nr(e.settings.inTangents,Array),outTangents:nr(e.settings.outTangents,Array)})}return i.type=e.ValueTypeName,i}InterpolantFactoryMethodDiscrete(e){return new Ll(this.times,this.values,this.getValueSize(),e)}InterpolantFactoryMethodLinear(e){return new Fl(this.times,this.values,this.getValueSize(),e)}InterpolantFactoryMethodSmooth(e){return new Bl(this.times,this.values,this.getValueSize(),e)}InterpolantFactoryMethodBezier(e){let t=new Ul(this.times,this.values,this.getValueSize(),e);return this.settings&&(t.inTangents=this.settings.inTangents,t.outTangents=this.settings.outTangents),t}setInterpolation(e){let t;switch(e){case fo:t=this.InterpolantFactoryMethodDiscrete;break;case Al:t=this.InterpolantFactoryMethodLinear;break;case vl:t=this.InterpolantFactoryMethodSmooth;break;case Ku:t=this.InterpolantFactoryMethodBezier;break}if(t===void 0){let i="unsupported interpolation for "+this.ValueTypeName+" keyframe track named "+this.name;if(this.createInterpolant===void 0)if(e!==this.DefaultInterpolation)this.setInterpolation(this.DefaultInterpolation);else throw new Error(i);return pe("KeyframeTrack:",i),this}return this.createInterpolant=t,this}getInterpolation(){switch(this.createInterpolant){case this.InterpolantFactoryMethodDiscrete:return fo;case this.InterpolantFactoryMethodLinear:return Al;case this.InterpolantFactoryMethodSmooth:return vl;case this.InterpolantFactoryMethodBezier:return Ku}}getValueSize(){return this.values.length/this.times.length}shift(e){if(e!==0){let t=this.times;for(let i=0,s=t.length;i!==s;++i)t[i]+=e}return this}scale(e){if(e!==1){let t=this.times;for(let i=0,s=t.length;i!==s;++i)t[i]*=e;Zu(this.settings)&&(Hp(this.settings.inTangents,e),Hp(this.settings.outTangents,e))}return this}trim(e,t){let i=this.times,s=i.length,r=0,o=s-1;for(;r!==s&&i[r]<e;)++r;for(;o!==-1&&i[o]>t;)--o;if(++o,r!==0||o!==s){r>=o&&(o=Math.max(o,1),r=o-1);let a=this.getValueSize();this.times=i.slice(r,o),this.values=this.values.slice(r*a,o*a)}return this}validate(){let e=!0,t=this.getValueSize();t-Math.floor(t)!==0&&(_e("KeyframeTrack: Invalid value size in track.",this),e=!1);let i=this.times,s=this.values,r=i.length;r===0&&(_e("KeyframeTrack: Track is empty.",this),e=!1);let o=null;for(let a=0;a!==r;a++){let l=i[a];if(typeof l=="number"&&isNaN(l)){_e("KeyframeTrack: Time is not a valid number.",this,a,l),e=!1;break}if(o!==null&&o>l){_e("KeyframeTrack: Out of order keys.",this,a,l,o),e=!1;break}o=l}if(s!==void 0&&Qp(s))for(let a=0,l=s.length;a!==l;++a){let c=s[a];if(isNaN(c)){_e("KeyframeTrack: Value is not a valid number.",this,a,c),e=!1;break}}return e}optimize(){let e=this.times.slice(),t=this.values.slice(),i=this.getValueSize(),s=this.getInterpolation()===vl,r=e.length-1,o=1;for(let a=1;a<r;++a){let l=!1,c=e[a],u=e[a+1];if(c!==u&&(a!==1||c!==e[0]))if(s)l=!0;else{let d=a*i,h=d-i,f=d+i;for(let g=0;g!==i;++g){let y=t[d+g];if(y!==t[h+g]||y!==t[f+g]){l=!0;break}}}if(l){if(a!==o){e[o]=e[a];let d=a*i,h=o*i;for(let f=0;f!==i;++f)t[h+f]=t[d+f]}++o}}if(r>0){e[o]=e[r];for(let a=r*i,l=o*i,c=0;c!==i;++c)t[l+c]=t[a+c];++o}return o!==e.length?(this.times=e.slice(0,o),this.values=t.slice(0,o*i)):(this.times=e,this.values=t),this}clone(){let e=this.times.slice(),t=this.values.slice(),i=this.constructor,s=new i(this.name,e,t);return s.createInterpolant=this.createInterpolant,Zu(this.settings)&&(s.settings={inTangents:this.settings.inTangents.slice(),outTangents:this.settings.outTangents.slice()}),s}};function Hp(n,e){for(let t=0,i=n.length;t!==i;t+=2)n[t]*=e}Nn.prototype.ValueTypeName="";Nn.prototype.TimeBufferType=Float32Array;Nn.prototype.ValueBufferType=Float32Array;Nn.prototype.DefaultInterpolation=Al;var Yi=class extends Nn{constructor(e,t,i){super(e,t,i)}};Yi.prototype.ValueTypeName="bool";Yi.prototype.ValueBufferType=Array;Yi.prototype.DefaultInterpolation=fo;Yi.prototype.InterpolantFactoryMethodLinear=void 0;Yi.prototype.InterpolantFactoryMethodSmooth=void 0;var Dl=class extends Nn{constructor(e,t,i,s){super(e,t,i,s)}};Dl.prototype.ValueTypeName="color";var Pl=class extends Nn{constructor(e,t,i,s){super(e,t,i,s)}};Pl.prototype.ValueTypeName="number";var Gl=class extends Hi{constructor(e,t,i,s){super(e,t,i,s)}interpolate_(e,t,i,s){let r=this.resultBuffer,o=this.sampleValues,a=this.valueSize,l=(i-t)/(s-t),c=e*a;for(let u=c+a;c!==u;c+=4)Xn.slerpFlat(r,0,o,c-a,o,c,l);return r}},Ao=class extends Nn{constructor(e,t,i,s){super(e,t,i,s)}InterpolantFactoryMethodLinear(e){return new Gl(this.times,this.values,this.getValueSize(),e)}};Ao.prototype.ValueTypeName="quaternion";Ao.prototype.InterpolantFactoryMethodSmooth=void 0;var Zi=class extends Nn{constructor(e,t,i){super(e,t,i)}};Zi.prototype.ValueTypeName="string";Zi.prototype.ValueBufferType=Array;Zi.prototype.DefaultInterpolation=fo;Zi.prototype.InterpolantFactoryMethodLinear=void 0;Zi.prototype.InterpolantFactoryMethodSmooth=void 0;var Vl=class extends Nn{constructor(e,t,i,s){super(e,t,i,s)}};Vl.prototype.ValueTypeName="vector";var Ol=class{constructor(e,t,i){let s=this,r=!1,o=0,a=0,l,c=[];this.onStart=void 0,this.onLoad=e,this.onProgress=t,this.onError=i,this._abortController=null,this.itemStart=function(u){a++,r===!1&&s.onStart!==void 0&&s.onStart(u,o,a),r=!0},this.itemEnd=function(u){o++,s.onProgress!==void 0&&s.onProgress(u,o,a),o===a&&(r=!1,s.onLoad!==void 0&&s.onLoad())},this.itemError=function(u){s.onError!==void 0&&s.onError(u)},this.resolveURL=function(u){return u=u.normalize("NFC"),l?l(u):u},this.setURLModifier=function(u){return l=u,this},this.addHandler=function(u,d){return c.push(u,d),this},this.removeHandler=function(u){let d=c.indexOf(u);return d!==-1&&c.splice(d,2),this},this.getHandler=function(u){for(let d=0,h=c.length;d<h;d+=2){let f=c[d],g=c[d+1];if(f.global&&(f.lastIndex=0),f.test(u))return g}return null},this.abort=function(){return this.abortController.abort(),this._abortController=null,this}}get abortController(){return this._abortController||(this._abortController=new AbortController),this._abortController}},sm=new Ol,To=class{constructor(e){this.manager=e!==void 0?e:sm,this.crossOrigin="anonymous",this.withCredentials=!1,this.path="",this.resourcePath="",this.requestHeader={},typeof __THREE_DEVTOOLS__<"u"&&__THREE_DEVTOOLS__.dispatchEvent(new CustomEvent("observe",{detail:this}))}load(){}loadAsync(e,t){let i=this;return new Promise(function(s,r){i.load(e,s,t,r)})}parse(){}setCrossOrigin(e){return this.crossOrigin=e,this}setWithCredentials(e){return this.withCredentials=e,this}setPath(e){return this.path=e,this}setResourcePath(e){return this.resourcePath=e,this}setRequestHeader(e){return this.requestHeader=e,this}abort(){return this}};To.DEFAULT_MATERIAL_NAME="__DEFAULT";var kl=class extends Jt{constructor(e,t=1){super(),this.isLight=!0,this.type="Light",this.color=new He(e),this.intensity=t}copy(e,t){return super.copy(e,t),this.color.copy(e.color),this.intensity=e.intensity,this}toJSON(e){let t=super.toJSON(e);return t.object.color=this.color.getHex(),t.object.intensity=this.intensity,t}};var Xu=new ct,Yp=new L,Zp=new L,zl=class{constructor(e){this.camera=e,this.intensity=1,this.bias=0,this.biasNode=null,this.normalBias=0,this.radius=1,this.blurSamples=8,this.mapSize=new ze(512,512),this.mapType=mn,this.map=null,this.mapPass=null,this.matrix=new ct,this.autoUpdate=!0,this.needsUpdate=!1,this._frustum=new ms,this._frameExtents=new ze(1,1),this._viewportCount=1,this._viewports=[new dt(0,0,1,1)]}getViewportCount(){return this._viewportCount}getCamera(){return this.camera}getFrustum(){return this._frustum}updateMatrices(e){let t=this.camera;Yp.setFromMatrixPosition(e.matrixWorld),t.position.copy(Yp),Zp.setFromMatrixPosition(e.target.matrixWorld),t.lookAt(Zp),t.updateMatrixWorld(),this._updateMatrix(t,this.matrix,this._frustum)}_updateMatrix(e,t,i,s){Xu.multiplyMatrices(e.projectionMatrix,e.matrixWorldInverse),i.setFromProjectionMatrix(Xu,e.coordinateSystem,e.reversedDepth);let r=this._frameExtents,o=s?s.z/r.x:1,a=s?s.w/r.y:1,l=s?s.x/r.x:0,c=s?s.y/r.y:0;e.coordinateSystem===li||e.reversedDepth?t.set(.5*o,0,0,.5*o+l,0,.5*a,0,.5*a+c,0,0,1,0,0,0,0,1):t.set(.5*o,0,0,.5*o+l,0,.5*a,0,.5*a+c,0,0,.5,.5,0,0,0,1),t.multiply(Xu)}getViewport(e){return this._viewports[e]}getFrameExtents(){return this._frameExtents}dispose(){this.map&&this.map.dispose(),this.mapPass&&this.mapPass.dispose()}copy(e){return this.camera=e.camera.clone(),this.intensity=e.intensity,this.bias=e.bias,this.radius=e.radius,this.autoUpdate=e.autoUpdate,this.needsUpdate=e.needsUpdate,this.normalBias=e.normalBias,this.blurSamples=e.blurSamples,this.mapSize.copy(e.mapSize),this.biasNode=e.biasNode,this}clone(){return new this.constructor().copy(this)}toJSON(){let e={};return e.intensity=this.intensity,e.bias=this.bias,e.normalBias=this.normalBias,e.radius=this.radius,e.blurSamples=this.blurSamples,e.mapSize=this.mapSize.toArray(),e.camera=this.camera.toJSON(!1).object,delete e.camera.matrix,e}},bl=new L,Sl=new Xn,ai=new L,Eo=class extends Jt{constructor(){super(),this.isCamera=!0,this.type="Camera",this.matrixWorldInverse=new ct,this.projectionMatrix=new ct,this.projectionMatrixInverse=new ct,this.coordinateSystem=hn,this._reversedDepth=!1}get reversedDepth(){return this._reversedDepth}copy(e,t){return super.copy(e,t),this.matrixWorldInverse.copy(e.matrixWorldInverse),this.projectionMatrix.copy(e.projectionMatrix),this.projectionMatrixInverse.copy(e.projectionMatrixInverse),this.coordinateSystem=e.coordinateSystem,this}getWorldDirection(e){return super.getWorldDirection(e).negate()}updateMatrixWorld(e){super.updateMatrixWorld(e),this.matrixWorld.decompose(bl,Sl,ai),ai.x===1&&ai.y===1&&ai.z===1?this.matrixWorldInverse.copy(this.matrixWorld).invert():this.matrixWorldInverse.compose(bl,Sl,ai.set(1,1,1)).invert()}updateWorldMatrix(e,t,i=!1){super.updateWorldMatrix(e,t,i),this.matrixWorld.decompose(bl,Sl,ai),ai.x===1&&ai.y===1&&ai.z===1?this.matrixWorldInverse.copy(this.matrixWorld).invert():this.matrixWorldInverse.compose(bl,Sl,ai.set(1,1,1)).invert()}clone(){return new this.constructor().copy(this)}},zi=new L,Xp=new ze,Kp=new ze,Kt=class extends Eo{constructor(e=50,t=1,i=.1,s=2e3){super(),this.isPerspectiveCamera=!0,this.type="PerspectiveCamera",this.fov=e,this.zoom=1,this.near=i,this.far=s,this.focus=10,this.aspect=t,this.view=null,this.filmGauge=35,this.filmOffset=0,this.updateProjectionMatrix()}copy(e,t){return super.copy(e,t),this.fov=e.fov,this.zoom=e.zoom,this.near=e.near,this.far=e.far,this.focus=e.focus,this.aspect=e.aspect,this.view=e.view===null?null:Object.assign({},e.view),this.filmGauge=e.filmGauge,this.filmOffset=e.filmOffset,this}setFocalLength(e){let t=.5*this.getFilmHeight()/e;this.fov=ds*2*Math.atan(t),this.updateProjectionMatrix()}getFocalLength(){let e=Math.tan(rr*.5*this.fov);return .5*this.getFilmHeight()/e}getEffectiveFOV(){return ds*2*Math.atan(Math.tan(rr*.5*this.fov)/this.zoom)}getFilmWidth(){return this.filmGauge*Math.min(this.aspect,1)}getFilmHeight(){return this.filmGauge/Math.max(this.aspect,1)}getViewBounds(e,t,i){zi.set(-1,-1,.5).applyMatrix4(this.projectionMatrixInverse),t.set(zi.x,zi.y).multiplyScalar(-e/zi.z),zi.set(1,1,.5).applyMatrix4(this.projectionMatrixInverse),i.set(zi.x,zi.y).multiplyScalar(-e/zi.z)}getViewSize(e,t){return this.getViewBounds(e,Xp,Kp),t.subVectors(Kp,Xp)}setViewOffset(e,t,i,s,r,o){this.aspect=e/t,this.view===null&&(this.view={enabled:!0,fullWidth:1,fullHeight:1,offsetX:0,offsetY:0,width:1,height:1}),this.view.enabled=!0,this.view.fullWidth=e,this.view.fullHeight=t,this.view.offsetX=i,this.view.offsetY=s,this.view.width=r,this.view.height=o,this.updateProjectionMatrix()}clearViewOffset(){this.view!==null&&(this.view.enabled=!1),this.updateProjectionMatrix()}updateProjectionMatrix(){let e=this.near,t=e*Math.tan(rr*.5*this.fov)/this.zoom,i=2*t,s=this.aspect*i,r=-.5*s,o=this.view;if(this.view!==null&&this.view.enabled){let l=o.fullWidth,c=o.fullHeight;r+=o.offsetX*s/l,t-=o.offsetY*i/c,s*=o.width/l,i*=o.height/c}let a=this.filmOffset;a!==0&&(r+=e*a/this.getFilmWidth()),this.projectionMatrix.makePerspective(r,r+s,t,t-i,e,this.far,this.coordinateSystem,this.reversedDepth),this.projectionMatrixInverse.copy(this.projectionMatrix).invert()}toJSON(e){let t=super.toJSON(e);return t.object.fov=this.fov,t.object.zoom=this.zoom,t.object.near=this.near,t.object.far=this.far,t.object.focus=this.focus,t.object.aspect=this.aspect,this.view!==null&&(t.object.view=Object.assign({},this.view)),t.object.filmGauge=this.filmGauge,t.object.filmOffset=this.filmOffset,t}};var _s=class extends Eo{constructor(e=-1,t=1,i=1,s=-1,r=.1,o=2e3){super(),this.isOrthographicCamera=!0,this.type="OrthographicCamera",this.zoom=1,this.view=null,this.left=e,this.right=t,this.top=i,this.bottom=s,this.near=r,this.far=o,this.updateProjectionMatrix()}copy(e,t){return super.copy(e,t),this.left=e.left,this.right=e.right,this.top=e.top,this.bottom=e.bottom,this.near=e.near,this.far=e.far,this.zoom=e.zoom,this.view=e.view===null?null:Object.assign({},e.view),this}setViewOffset(e,t,i,s,r,o){this.view===null&&(this.view={enabled:!0,fullWidth:1,fullHeight:1,offsetX:0,offsetY:0,width:1,height:1}),this.view.enabled=!0,this.view.fullWidth=e,this.view.fullHeight=t,this.view.offsetX=i,this.view.offsetY=s,this.view.width=r,this.view.height=o,this.updateProjectionMatrix()}clearViewOffset(){this.view!==null&&(this.view.enabled=!1),this.updateProjectionMatrix()}updateProjectionMatrix(){let e=(this.right-this.left)/(2*this.zoom),t=(this.top-this.bottom)/(2*this.zoom),i=(this.right+this.left)/2,s=(this.top+this.bottom)/2,r=i-e,o=i+e,a=s+t,l=s-t;if(this.view!==null&&this.view.enabled){let c=(this.right-this.left)/this.view.fullWidth/this.zoom,u=(this.top-this.bottom)/this.view.fullHeight/this.zoom;r+=c*this.view.offsetX,o=r+c*this.view.width,a-=u*this.view.offsetY,l=a-u*this.view.height}this.projectionMatrix.makeOrthographic(r,o,a,l,this.near,this.far,this.coordinateSystem,this.reversedDepth),this.projectionMatrixInverse.copy(this.projectionMatrix).invert()}toJSON(e){let t=super.toJSON(e);return t.object.zoom=this.zoom,t.object.left=this.left,t.object.right=this.right,t.object.top=this.top,t.object.bottom=this.bottom,t.object.near=this.near,t.object.far=this.far,this.view!==null&&(t.object.view=Object.assign({},this.view)),t}},Qu=class extends zl{constructor(){super(new _s(-5,5,5,-5,.5,500)),this.isDirectionalLightShadow=!0}},vr=class extends kl{constructor(e,t){super(e,t),this.isDirectionalLight=!0,this.type="DirectionalLight",this.position.copy(Jt.DEFAULT_UP),this.updateMatrix(),this.target=new Jt,this.shadow=new Qu}dispose(){super.dispose(),this.shadow.dispose()}copy(e){return super.copy(e),this.target=e.target.clone(),this.shadow=e.shadow.clone(),this}toJSON(e){let t=super.toJSON(e);return t.object.shadow=this.shadow.toJSON(),t.object.target=this.target.uuid,t}};var ir=-90,sr=1,Mo=class extends Jt{constructor(e,t,i){super(),this.type="CubeCamera",this.renderTarget=i,this.coordinateSystem=null,this.activeMipmapLevel=0;let s=new Kt(ir,sr,e,t);s.layers=this.layers,this.add(s);let r=new Kt(ir,sr,e,t);r.layers=this.layers,this.add(r);let o=new Kt(ir,sr,e,t);o.layers=this.layers,this.add(o);let a=new Kt(ir,sr,e,t);a.layers=this.layers,this.add(a);let l=new Kt(ir,sr,e,t);l.layers=this.layers,this.add(l);let c=new Kt(ir,sr,e,t);c.layers=this.layers,this.add(c)}updateCoordinateSystem(){let e=this.coordinateSystem,t=this.children.concat(),[i,s,r,o,a,l]=t;for(let c of t)this.remove(c);if(e===hn)i.up.set(0,1,0),i.lookAt(1,0,0),s.up.set(0,1,0),s.lookAt(-1,0,0),r.up.set(0,0,-1),r.lookAt(0,1,0),o.up.set(0,0,1),o.lookAt(0,-1,0),a.up.set(0,1,0),a.lookAt(0,0,1),l.up.set(0,1,0),l.lookAt(0,0,-1);else if(e===li)i.up.set(0,-1,0),i.lookAt(-1,0,0),s.up.set(0,-1,0),s.lookAt(1,0,0),r.up.set(0,0,1),r.lookAt(0,1,0),o.up.set(0,0,-1),o.lookAt(0,-1,0),a.up.set(0,-1,0),a.lookAt(0,0,1),l.up.set(0,-1,0),l.lookAt(0,0,-1);else throw new Error("THREE.CubeCamera.updateCoordinateSystem(): Invalid coordinate system: "+e);for(let c of t)this.add(c),c.updateMatrixWorld()}update(e,t){this.parent===null&&this.updateMatrixWorld();let{renderTarget:i,activeMipmapLevel:s}=this;this.coordinateSystem!==e.coordinateSystem&&(this.coordinateSystem=e.coordinateSystem,this.updateCoordinateSystem());let[r,o,a,l,c,u]=this.children,d=e.getRenderTarget(),h=e.getActiveCubeFace(),f=e.getActiveMipmapLevel(),g=e.xr.enabled;e.xr.enabled=!1;let y=i.texture.generateMipmaps;i.texture.generateMipmaps=!1;let m=!1;e.isWebGLRenderer===!0?m=e.state.buffers.depth.getReversed():m=e.reversedDepthBuffer,e.setRenderTarget(i,0,s),m&&e.autoClear===!1&&e.clearDepth(),e.render(t,r),e.setRenderTarget(i,1,s),m&&e.autoClear===!1&&e.clearDepth(),e.render(t,o),e.setRenderTarget(i,2,s),m&&e.autoClear===!1&&e.clearDepth(),e.render(t,a),e.setRenderTarget(i,3,s),m&&e.autoClear===!1&&e.clearDepth(),e.render(t,l),e.setRenderTarget(i,4,s),m&&e.autoClear===!1&&e.clearDepth(),e.render(t,c),i.texture.generateMipmaps=y,e.setRenderTarget(i,5,s),m&&e.autoClear===!1&&e.clearDepth(),e.render(t,u),e.setRenderTarget(d,h,f),e.xr.enabled=g,i.texture.needsPMREMUpdate=!0}},Ro=class extends Kt{constructor(e=[]){super(),this.isArrayCamera=!0,this.isMultiViewCamera=!1,this.cameras=e}};var Dh="\\[\\]\\.:\\/",gy=new RegExp("["+Dh+"]","g"),Ph="[^"+Dh+"]",xy="[^"+Dh.replace("\\.","")+"]",yy=/((?:WC+[\/:])*)/.source.replace("WC",Ph),_y=/(WCOD+)?/.source.replace("WCOD",xy),by=/(?:\.(WC+)(?:\[(.+)\])?)?/.source.replace("WC",Ph),Sy=/\.(WC+)(?:\[(.+)\])?/.source.replace("WC",Ph),vy=new RegExp("^"+yy+_y+by+Sy+"$"),Ay=["material","materials","bones","map"],ju=class{constructor(e,t,i){let s=i||At.parseTrackName(t);this._targetGroup=e,this._bindings=e.subscribe_(t,s)}getValue(e,t){this.bind();let i=this._targetGroup.nCachedObjects_,s=this._bindings[i];s!==void 0&&s.getValue(e,t)}setValue(e,t){let i=this._bindings;for(let s=this._targetGroup.nCachedObjects_,r=i.length;s!==r;++s)i[s].setValue(e,t)}bind(){let e=this._bindings;for(let t=this._targetGroup.nCachedObjects_,i=e.length;t!==i;++t)e[t].bind()}unbind(){let e=this._bindings;for(let t=this._targetGroup.nCachedObjects_,i=e.length;t!==i;++t)e[t].unbind()}},At=class n{constructor(e,t,i){this.path=t,this.parsedPath=i||n.parseTrackName(t),this.node=n.findNode(e,this.parsedPath.nodeName),this.rootNode=e,this.getValue=this._getValue_unbound,this.setValue=this._setValue_unbound}static create(e,t,i){return e&&e.isAnimationObjectGroup?new n.Composite(e,t,i):new n(e,t,i)}static sanitizeNodeName(e){return e.replace(/\s/g,"_").replace(gy,"")}static parseTrackName(e){let t=vy.exec(e);if(t===null)throw new Error("THREE.PropertyBinding: Cannot parse trackName: "+e);let i={nodeName:t[2],objectName:t[3],objectIndex:t[4],propertyName:t[5],propertyIndex:t[6]},s=i.nodeName&&i.nodeName.lastIndexOf(".");if(s!==void 0&&s!==-1){let r=i.nodeName.substring(s+1);Ay.indexOf(r)!==-1&&(i.nodeName=i.nodeName.substring(0,s),i.objectName=r)}if(i.propertyName===null||i.propertyName.length===0)throw new Error("THREE.PropertyBinding: can not parse propertyName from trackName: "+e);return i}static findNode(e,t){if(t===void 0||t===""||t==="."||t===-1||t===e.name||t===e.uuid)return e;if(e.skeleton){let i=e.skeleton.getBoneByName(t);if(i!==void 0)return i}if(e.children){let i=function(r){for(let o=0;o<r.length;o++){let a=r[o];if(a.name===t||a.uuid===t)return a;let l=i(a.children);if(l)return l}return null},s=i(e.children);if(s)return s}return null}_getValue_unavailable(){}_setValue_unavailable(){}_getValue_direct(e,t){e[t]=this.targetObject[this.propertyName]}_getValue_array(e,t){let i=this.resolvedProperty;for(let s=0,r=i.length;s!==r;++s)e[t++]=i[s]}_getValue_arrayElement(e,t){e[t]=this.resolvedProperty[this.propertyIndex]}_getValue_toArray(e,t){this.resolvedProperty.toArray(e,t)}_setValue_direct(e,t){this.targetObject[this.propertyName]=e[t]}_setValue_direct_setNeedsUpdate(e,t){this.targetObject[this.propertyName]=e[t],this.targetObject.needsUpdate=!0}_setValue_direct_setMatrixWorldNeedsUpdate(e,t){this.targetObject[this.propertyName]=e[t],this.targetObject.matrixWorldNeedsUpdate=!0}_setValue_array(e,t){let i=this.resolvedProperty;for(let s=0,r=i.length;s!==r;++s)i[s]=e[t++]}_setValue_array_setNeedsUpdate(e,t){let i=this.resolvedProperty;for(let s=0,r=i.length;s!==r;++s)i[s]=e[t++];this.targetObject.needsUpdate=!0}_setValue_array_setMatrixWorldNeedsUpdate(e,t){let i=this.resolvedProperty;for(let s=0,r=i.length;s!==r;++s)i[s]=e[t++];this.targetObject.matrixWorldNeedsUpdate=!0}_setValue_arrayElement(e,t){this.resolvedProperty[this.propertyIndex]=e[t]}_setValue_arrayElement_setNeedsUpdate(e,t){this.resolvedProperty[this.propertyIndex]=e[t],this.targetObject.needsUpdate=!0}_setValue_arrayElement_setMatrixWorldNeedsUpdate(e,t){this.resolvedProperty[this.propertyIndex]=e[t],this.targetObject.matrixWorldNeedsUpdate=!0}_setValue_fromArray(e,t){this.resolvedProperty.fromArray(e,t)}_setValue_fromArray_setNeedsUpdate(e,t){this.resolvedProperty.fromArray(e,t),this.targetObject.needsUpdate=!0}_setValue_fromArray_setMatrixWorldNeedsUpdate(e,t){this.resolvedProperty.fromArray(e,t),this.targetObject.matrixWorldNeedsUpdate=!0}_getValue_unbound(e,t){this.bind(),this.getValue(e,t)}_setValue_unbound(e,t){this.bind(),this.setValue(e,t)}bind(){let e=this.node,t=this.parsedPath,i=t.objectName,s=t.propertyName,r=t.propertyIndex;if(e||(e=n.findNode(this.rootNode,t.nodeName),this.node=e),this.getValue=this._getValue_unavailable,this.setValue=this._setValue_unavailable,!e){pe("PropertyBinding: No target node found for track: "+this.path+".");return}if(i){let c=t.objectIndex;switch(i){case"materials":if(!e.material){_e("PropertyBinding: Can not bind to material as node does not have a material.",this);return}if(!e.material.materials){_e("PropertyBinding: Can not bind to material.materials as node.material does not have a materials array.",this);return}e=e.material.materials;break;case"bones":if(!e.skeleton){_e("PropertyBinding: Can not bind to bones as node does not have a skeleton.",this);return}e=e.skeleton.bones;for(let u=0;u<e.length;u++)if(e[u].name===c){c=u;break}break;case"map":if("map"in e){e=e.map;break}if(!e.material){_e("PropertyBinding: Can not bind to material as node does not have a material.",this);return}if(!e.material.map){_e("PropertyBinding: Can not bind to material.map as node.material does not have a map.",this);return}e=e.material.map;break;default:if(e[i]===void 0){_e("PropertyBinding: Can not bind to objectName of node undefined.",this);return}e=e[i]}if(c!==void 0){if(e[c]===void 0){_e("PropertyBinding: Trying to bind to objectIndex of objectName, but is undefined.",this,e);return}e=e[c]}}let o=e[s];if(o===void 0){let c=t.nodeName;_e("PropertyBinding: Trying to update property for track: "+c+"."+s+" but it wasn't found.",e);return}let a=this.Versioning.None;this.targetObject=e,e.isMaterial===!0?a=this.Versioning.NeedsUpdate:e.isObject3D===!0&&(a=this.Versioning.MatrixWorldNeedsUpdate);let l=this.BindingType.Direct;if(r!==void 0){if(s==="morphTargetInfluences"){if(!e.geometry){_e("PropertyBinding: Can not bind to morphTargetInfluences because node does not have a geometry.",this);return}if(!e.geometry.morphAttributes){_e("PropertyBinding: Can not bind to morphTargetInfluences because node does not have a geometry.morphAttributes.",this);return}e.morphTargetDictionary[r]!==void 0&&(r=e.morphTargetDictionary[r])}l=this.BindingType.ArrayElement,this.resolvedProperty=o,this.propertyIndex=r}else o.fromArray!==void 0&&o.toArray!==void 0?(l=this.BindingType.HasFromToArray,this.resolvedProperty=o):Array.isArray(o)?(l=this.BindingType.EntireArray,this.resolvedProperty=o):this.propertyName=s;this.getValue=this.GetterByBindingType[l],this.setValue=this.SetterByBindingTypeAndVersioning[l][a]}unbind(){this.node=null,this.getValue=this._getValue_unbound,this.setValue=this._setValue_unbound}};At.Composite=ju;At.prototype.BindingType={Direct:0,EntireArray:1,ArrayElement:2,HasFromToArray:3};At.prototype.Versioning={None:0,NeedsUpdate:1,MatrixWorldNeedsUpdate:2};At.prototype.GetterByBindingType=[At.prototype._getValue_direct,At.prototype._getValue_array,At.prototype._getValue_arrayElement,At.prototype._getValue_toArray];At.prototype.SetterByBindingTypeAndVersioning=[[At.prototype._setValue_direct,At.prototype._setValue_direct_setNeedsUpdate,At.prototype._setValue_direct_setMatrixWorldNeedsUpdate],[At.prototype._setValue_array,At.prototype._setValue_array_setNeedsUpdate,At.prototype._setValue_array_setMatrixWorldNeedsUpdate],[At.prototype._setValue_arrayElement,At.prototype._setValue_arrayElement_setNeedsUpdate,At.prototype._setValue_arrayElement_setMatrixWorldNeedsUpdate],[At.prototype._setValue_fromArray,At.prototype._setValue_fromArray_setNeedsUpdate,At.prototype._setValue_fromArray_setMatrixWorldNeedsUpdate]];var JC=new Float32Array(1);var $p=new ct,wo=class{constructor(e,t,i=0,s=1/0){this.ray=new yr(e,t),this.near=i,this.far=s,this.camera=null,this.layers=new pr,this.params={Mesh:{},Line:{threshold:1},LOD:{},Points:{threshold:1},Sprite:{}}}set(e,t){this.ray.set(e,t)}setFromCamera(e,t){t.isPerspectiveCamera?(this.ray.origin.setFromMatrixPosition(t.matrixWorld),this.ray.direction.set(e.x,e.y,.5).unproject(t).sub(this.ray.origin).normalize(),this.camera=t):t.isOrthographicCamera?(this.ray.origin.set(e.x,e.y,t.projectionMatrix.elements[14]).unproject(t),this.ray.direction.set(0,0,-1).transformDirection(t.matrixWorld),this.camera=t):_e("Raycaster: Unsupported camera type: "+t.type)}setFromXRController(e){return $p.identity().extractRotation(e.matrixWorld),this.ray.origin.setFromMatrixPosition(e.matrixWorld),this.ray.direction.set(0,0,-1).applyMatrix4($p),this}intersectObject(e,t=!0,i=[]){return eh(e,this,i,t),i.sort(Jp),i}intersectObjects(e,t=!0,i=[]){for(let s=0,r=e.length;s<r;s++)eh(e[s],this,i,t);return i.sort(Jp),i}};function Jp(n,e){return n.distance-e.distance}function eh(n,e,t,i){let s=!0;if(n.layers.test(e.layers)&&n.raycast(e,t)===!1&&(s=!1),s===!0&&i===!0){let r=n.children;for(let o=0,a=r.length;o<a;o++)eh(r[o],e,t,!0)}}var Co=class n{static{n.prototype.isMatrix2=!0}constructor(e,t,i,s){this.elements=[1,0,0,1],e!==void 0&&this.set(e,t,i,s)}identity(){return this.set(1,0,0,1),this}fromArray(e,t=0){for(let i=0;i<4;i++)this.elements[i]=e[i+t];return this}set(e,t,i,s){let r=this.elements;return r[0]=e,r[2]=t,r[1]=i,r[3]=s,this}};function dc(n,e,t,i){let s=Ty(i);switch(t){case Lo:return n*e;case Do:return n*e/s.components*s.byteLength;case Rr:return n*e/s.components*s.byteLength;case Qn:return n*e*2/s.components*s.byteLength;case wr:return n*e*2/s.components*s.byteLength;case Uo:return n*e*3/s.components*s.byteLength;case gn:return n*e*4/s.components*s.byteLength;case Po:return n*e*4/s.components*s.byteLength;case Cr:case Nr:return Math.floor((n+3)/4)*Math.floor((e+3)/4)*8;case Ir:case Br:return Math.floor((n+3)/4)*Math.floor((e+3)/4)*16;case Vo:case ko:return Math.max(n,16)*Math.max(e,8)/4;case Go:case Oo:return Math.max(n,8)*Math.max(e,8)/2;case zo:case Wo:case Yo:case Zo:return Math.floor((n+3)/4)*Math.floor((e+3)/4)*8;case Ho:case Ms:case Xo:return Math.floor((n+3)/4)*Math.floor((e+3)/4)*16;case Ko:return Math.floor((n+3)/4)*Math.floor((e+3)/4)*16;case $o:return Math.floor((n+4)/5)*Math.floor((e+3)/4)*16;case Jo:return Math.floor((n+4)/5)*Math.floor((e+4)/5)*16;case qo:return Math.floor((n+5)/6)*Math.floor((e+4)/5)*16;case Qo:return Math.floor((n+5)/6)*Math.floor((e+5)/6)*16;case jo:return Math.floor((n+7)/8)*Math.floor((e+4)/5)*16;case ea:return Math.floor((n+7)/8)*Math.floor((e+5)/6)*16;case ta:return Math.floor((n+7)/8)*Math.floor((e+7)/8)*16;case na:return Math.floor((n+9)/10)*Math.floor((e+4)/5)*16;case ia:return Math.floor((n+9)/10)*Math.floor((e+5)/6)*16;case sa:return Math.floor((n+9)/10)*Math.floor((e+7)/8)*16;case ra:return Math.floor((n+9)/10)*Math.floor((e+9)/10)*16;case oa:return Math.floor((n+11)/12)*Math.floor((e+9)/10)*16;case aa:return Math.floor((n+11)/12)*Math.floor((e+11)/12)*16;case la:case ca:case ua:return Math.ceil(n/4)*Math.ceil(e/4)*16;case ha:case da:return Math.ceil(n/4)*Math.ceil(e/4)*8;case Rs:case fa:return Math.ceil(n/4)*Math.ceil(e/4)*16}throw new Error(`Unable to determine texture byte length for ${t} format.`)}function Ty(n){switch(n){case mn:case nc:return{byteLength:1,components:1};case As:case ic:case Sn:return{byteLength:2,components:1};case Bo:case Fo:return{byteLength:2,components:4};case bn:case Ts:case Dn:return{byteLength:4,components:1};case sc:case rc:return{byteLength:4,components:3}}throw new Error(`THREE.TextureUtils: Unknown texture type ${n}.`)}typeof __THREE_DEVTOOLS__<"u"&&__THREE_DEVTOOLS__.dispatchEvent(new CustomEvent("register",{detail:{revision:"186"}}));typeof window<"u"&&(window.__THREE__?pe("WARNING: Multiple instances of Three.js being imported."):window.__THREE__="186");function Rm(){let n=null,e=!1,t=null,i=null;function s(r,o){i=n.requestAnimationFrame(s),t(r,o)}return{start:function(){e!==!0&&t!==null&&n!==null&&(i=n.requestAnimationFrame(s),e=!0)},stop:function(){n!==null&&n.cancelAnimationFrame(i),e=!1},setAnimationLoop:function(r){t=r},setContext:function(r){n=r}}}function g_(n){let e=new WeakMap;function t(a,l){let c=a.array,u=a.usage,d=c.byteLength,h=n.createBuffer();n.bindBuffer(l,h),n.bufferData(l,c,u),a.onUploadCallback();let f;if(c instanceof Float32Array)f=n.FLOAT;else if(typeof Float16Array<"u"&&c instanceof Float16Array)f=n.HALF_FLOAT;else if(c instanceof Uint16Array)a.isFloat16BufferAttribute?f=n.HALF_FLOAT:f=n.UNSIGNED_SHORT;else if(c instanceof Int16Array)f=n.SHORT;else if(c instanceof Uint32Array)f=n.UNSIGNED_INT;else if(c instanceof Int32Array)f=n.INT;else if(c instanceof Int8Array)f=n.BYTE;else if(c instanceof Uint8Array)f=n.UNSIGNED_BYTE;else if(c instanceof Uint8ClampedArray)f=n.UNSIGNED_BYTE;else throw new Error("THREE.WebGLAttributes: Unsupported buffer data format: "+c);return{buffer:h,type:f,bytesPerElement:c.BYTES_PER_ELEMENT,version:a.version,size:d}}function i(a,l,c){let u=l.array,d=l.updateRanges;if(n.bindBuffer(c,a),d.length===0)n.bufferSubData(c,0,u);else{d.sort((f,g)=>f.start-g.start);let h=0;for(let f=1;f<d.length;f++){let g=d[h],y=d[f];y.start<=g.start+g.count+1?g.count=Math.max(g.count,y.start+y.count-g.start):(++h,d[h]=y)}d.length=h+1;for(let f=0,g=d.length;f<g;f++){let y=d[f];n.bufferSubData(c,y.start*u.BYTES_PER_ELEMENT,u,y.start,y.count)}l.clearUpdateRanges()}l.onUploadCallback()}function s(a){return a.isInterleavedBufferAttribute&&(a=a.data),e.get(a)}function r(a){a.isInterleavedBufferAttribute&&(a=a.data);let l=e.get(a);l&&(n.deleteBuffer(l.buffer),e.delete(a))}function o(a,l){if(a.isInterleavedBufferAttribute&&(a=a.data),a.isGLBufferAttribute){let u=e.get(a);(!u||u.version<a.version)&&e.set(a,{buffer:a.buffer,type:a.type,bytesPerElement:a.elementSize,version:a.version});return}let c=e.get(a);if(c===void 0)e.set(a,t(a,l));else if(c.version<a.version){if(c.size!==a.array.byteLength)throw new Error("THREE.WebGLAttributes: The size of the buffer attribute's array buffer does not match the original size. Resizing buffer attributes is not supported.");i(c.buffer,a,l),c.version=a.version}}return{get:s,remove:r,update:o}}var x_=`#ifdef USE_ALPHAHASH
	if ( diffuseColor.a < getAlphaHashThreshold( vPosition ) ) discard;
#endif`,y_=`#ifdef USE_ALPHAHASH
	const float ALPHA_HASH_SCALE = 0.05;
	float hash2D( vec2 value ) {
		return fract( 1.0e4 * sin( 17.0 * value.x + 0.1 * value.y ) * ( 0.1 + abs( sin( 13.0 * value.y + value.x ) ) ) );
	}
	float hash3D( vec3 value ) {
		return hash2D( vec2( hash2D( value.xy ), value.z ) );
	}
	float getAlphaHashThreshold( vec3 position ) {
		float maxDeriv = max(
			length( dFdx( position.xyz ) ),
			length( dFdy( position.xyz ) )
		);
		float pixScale = 1.0 / ( ALPHA_HASH_SCALE * maxDeriv );
		vec2 pixScales = vec2(
			exp2( floor( log2( pixScale ) ) ),
			exp2( ceil( log2( pixScale ) ) )
		);
		vec2 alpha = vec2(
			hash3D( floor( pixScales.x * position.xyz ) ),
			hash3D( floor( pixScales.y * position.xyz ) )
		);
		float lerpFactor = fract( log2( pixScale ) );
		float x = ( 1.0 - lerpFactor ) * alpha.x + lerpFactor * alpha.y;
		float a = min( lerpFactor, 1.0 - lerpFactor );
		vec3 cases = vec3(
			x * x / ( 2.0 * a * ( 1.0 - a ) ),
			( x - 0.5 * a ) / ( 1.0 - a ),
			1.0 - ( ( 1.0 - x ) * ( 1.0 - x ) / ( 2.0 * a * ( 1.0 - a ) ) )
		);
		float threshold = ( x < ( 1.0 - a ) )
			? ( ( x < a ) ? cases.x : cases.y )
			: cases.z;
		return clamp( threshold , 1.0e-6, 1.0 );
	}
#endif`,__=`#ifdef USE_ALPHAMAP
	diffuseColor.a *= texture2D( alphaMap, vAlphaMapUv ).g;
#endif`,b_=`#ifdef USE_ALPHAMAP
	uniform sampler2D alphaMap;
#endif`,S_=`#ifdef USE_ALPHATEST
	#ifdef ALPHA_TO_COVERAGE
	diffuseColor.a = smoothstep( alphaTest, alphaTest + fwidth( diffuseColor.a ), diffuseColor.a );
	if ( diffuseColor.a == 0.0 ) discard;
	#else
	if ( diffuseColor.a < alphaTest ) discard;
	#endif
#endif`,v_=`#ifdef USE_ALPHATEST
	uniform float alphaTest;
#endif`,A_=`#ifdef USE_AOMAP
	float ambientOcclusion = ( texture2D( aoMap, vAoMapUv ).r - 1.0 ) * aoMapIntensity + 1.0;
	reflectedLight.indirectDiffuse *= ambientOcclusion;
	#if defined( USE_CLEARCOAT ) 
		clearcoatSpecularIndirect *= ambientOcclusion;
	#endif
	#if defined( USE_SHEEN ) 
		sheenSpecularIndirect *= ambientOcclusion;
	#endif
	#if defined( USE_ENVMAP ) && defined( STANDARD )
		float dotNV = saturate( dot( geometryNormal, geometryViewDir ) );
		reflectedLight.indirectSpecular *= computeSpecularOcclusion( dotNV, ambientOcclusion, material.roughness );
	#endif
#endif`,T_=`#ifdef USE_AOMAP
	uniform sampler2D aoMap;
	uniform float aoMapIntensity;
#endif`,E_=`#ifdef USE_BATCHING
	#if ! defined( GL_ANGLE_multi_draw )
	#define gl_DrawID _gl_DrawID
	uniform int _gl_DrawID;
	#endif
	uniform highp sampler2D batchingTexture;
	uniform highp usampler2D batchingIdTexture;
	mat4 getBatchingMatrix( const in float i ) {
		int size = textureSize( batchingTexture, 0 ).x;
		int j = int( i ) * 4;
		int x = j % size;
		int y = j / size;
		vec4 v1 = texelFetch( batchingTexture, ivec2( x, y ), 0 );
		vec4 v2 = texelFetch( batchingTexture, ivec2( x + 1, y ), 0 );
		vec4 v3 = texelFetch( batchingTexture, ivec2( x + 2, y ), 0 );
		vec4 v4 = texelFetch( batchingTexture, ivec2( x + 3, y ), 0 );
		return mat4( v1, v2, v3, v4 );
	}
	float getIndirectIndex( const in int i ) {
		int size = textureSize( batchingIdTexture, 0 ).x;
		int x = i % size;
		int y = i / size;
		return float( texelFetch( batchingIdTexture, ivec2( x, y ), 0 ).r );
	}
#endif
#ifdef USE_BATCHING_COLOR
	uniform sampler2D batchingColorTexture;
	vec4 getBatchingColor( const in float i ) {
		int size = textureSize( batchingColorTexture, 0 ).x;
		int j = int( i );
		int x = j % size;
		int y = j / size;
		return texelFetch( batchingColorTexture, ivec2( x, y ), 0 );
	}
#endif`,M_=`#ifdef USE_BATCHING
	mat4 batchingMatrix = getBatchingMatrix( getIndirectIndex( gl_DrawID ) );
#endif`,R_=`vec3 transformed = vec3( position );
#ifdef USE_ALPHAHASH
	vPosition = vec3( position );
#endif`,w_=`vec3 objectNormal = vec3( normal );
#ifdef USE_TANGENT
	vec3 objectTangent = vec3( tangent.xyz );
#endif`,C_=`float G_BlinnPhong_Implicit( ) {
	return 0.25;
}
float D_BlinnPhong( const in float shininess, const in float dotNH ) {
	return RECIPROCAL_PI * ( shininess * 0.5 + 1.0 ) * pow( dotNH, shininess );
}
vec3 BRDF_BlinnPhong( const in vec3 lightDir, const in vec3 viewDir, const in vec3 normal, const in vec3 specularColor, const in float shininess ) {
	vec3 halfDir = normalize( lightDir + viewDir );
	float dotNH = saturate( dot( normal, halfDir ) );
	float dotVH = saturate( dot( viewDir, halfDir ) );
	vec3 F = F_Schlick( specularColor, 1.0, dotVH );
	float G = G_BlinnPhong_Implicit( );
	float D = D_BlinnPhong( shininess, dotNH );
	return F * ( G * D );
} // validated`,N_=`#ifdef USE_IRIDESCENCE
	const mat3 XYZ_TO_REC709 = mat3(
		 3.2404542, -0.9692660,  0.0556434,
		-1.5371385,  1.8760108, -0.2040259,
		-0.4985314,  0.0415560,  1.0572252
	);
	vec3 Fresnel0ToIor( vec3 fresnel0 ) {
		vec3 sqrtF0 = sqrt( fresnel0 );
		return ( vec3( 1.0 ) + sqrtF0 ) / ( vec3( 1.0 ) - sqrtF0 );
	}
	vec3 IorToFresnel0( vec3 transmittedIor, float incidentIor ) {
		return pow2( ( transmittedIor - vec3( incidentIor ) ) / ( transmittedIor + vec3( incidentIor ) ) );
	}
	float IorToFresnel0( float transmittedIor, float incidentIor ) {
		return pow2( ( transmittedIor - incidentIor ) / ( transmittedIor + incidentIor ));
	}
	vec3 evalSensitivity( float OPD, vec3 shift ) {
		float phase = 2.0 * PI * OPD * 1.0e-9;
		vec3 val = vec3( 5.4856e-13, 4.4201e-13, 5.2481e-13 );
		vec3 pos = vec3( 1.6810e+06, 1.7953e+06, 2.2084e+06 );
		vec3 var = vec3( 4.3278e+09, 9.3046e+09, 6.6121e+09 );
		vec3 xyz = val * sqrt( 2.0 * PI * var ) * cos( pos * phase + shift ) * exp( - pow2( phase ) * var );
		xyz.x += 9.7470e-14 * sqrt( 2.0 * PI * 4.5282e+09 ) * cos( 2.2399e+06 * phase + shift[ 0 ] ) * exp( - 4.5282e+09 * pow2( phase ) );
		xyz /= 1.0685e-7;
		vec3 rgb = XYZ_TO_REC709 * xyz;
		return rgb;
	}
	vec3 evalIridescence( float outsideIOR, float eta2, float cosTheta1, float thinFilmThickness, vec3 baseF0 ) {
		vec3 I;
		float iridescenceIOR = mix( outsideIOR, eta2, smoothstep( 0.0, 0.03, thinFilmThickness ) );
		float sinTheta2Sq = pow2( outsideIOR / iridescenceIOR ) * ( 1.0 - pow2( cosTheta1 ) );
		float cosTheta2Sq = 1.0 - sinTheta2Sq;
		if ( cosTheta2Sq < 0.0 ) {
			return vec3( 1.0 );
		}
		float cosTheta2 = sqrt( cosTheta2Sq );
		float R0 = IorToFresnel0( iridescenceIOR, outsideIOR );
		float R12 = F_Schlick( R0, 1.0, cosTheta1 );
		float T121 = 1.0 - R12;
		float phi12 = 0.0;
		if ( iridescenceIOR < outsideIOR ) phi12 = PI;
		float phi21 = PI - phi12;
		vec3 baseIOR = Fresnel0ToIor( clamp( baseF0, 0.0, 0.9999 ) );		vec3 R1 = IorToFresnel0( baseIOR, iridescenceIOR );
		vec3 R23 = F_Schlick( R1, 1.0, cosTheta2 );
		vec3 phi23 = vec3( 0.0 );
		if ( baseIOR[ 0 ] < iridescenceIOR ) phi23[ 0 ] = PI;
		if ( baseIOR[ 1 ] < iridescenceIOR ) phi23[ 1 ] = PI;
		if ( baseIOR[ 2 ] < iridescenceIOR ) phi23[ 2 ] = PI;
		float OPD = 2.0 * iridescenceIOR * thinFilmThickness * cosTheta2;
		vec3 phi = vec3( phi21 ) + phi23;
		vec3 R123 = clamp( R12 * R23, 1e-5, 0.9999 );
		vec3 r123 = sqrt( R123 );
		vec3 Rs = pow2( T121 ) * R23 / ( vec3( 1.0 ) - R123 );
		vec3 C0 = R12 + Rs;
		I = C0;
		vec3 Cm = Rs - T121;
		for ( int m = 1; m <= 2; ++ m ) {
			Cm *= r123;
			vec3 Sm = 2.0 * evalSensitivity( float( m ) * OPD, float( m ) * phi );
			I += Cm * Sm;
		}
		return max( I, vec3( 0.0 ) );
	}
#endif`,I_=`#ifdef USE_BUMPMAP
	uniform sampler2D bumpMap;
	uniform float bumpScale;
	vec2 dHdxy_fwd() {
		vec2 dSTdx = dFdx( vBumpMapUv );
		vec2 dSTdy = dFdy( vBumpMapUv );
		float Hll = bumpScale * texture2D( bumpMap, vBumpMapUv ).x;
		float dBx = bumpScale * texture2D( bumpMap, vBumpMapUv + dSTdx ).x - Hll;
		float dBy = bumpScale * texture2D( bumpMap, vBumpMapUv + dSTdy ).x - Hll;
		return vec2( dBx, dBy );
	}
	vec3 perturbNormalArb( vec3 surf_pos, vec3 surf_norm, vec2 dHdxy, float faceDirection ) {
		vec3 vSigmaX = normalize( dFdx( surf_pos.xyz ) );
		vec3 vSigmaY = normalize( dFdy( surf_pos.xyz ) );
		vec3 vN = surf_norm;
		vec3 R1 = cross( vSigmaY, vN );
		vec3 R2 = cross( vN, vSigmaX );
		float fDet = dot( vSigmaX, R1 ) * faceDirection;
		vec3 vGrad = sign( fDet ) * ( dHdxy.x * R1 + dHdxy.y * R2 );
		return normalize( abs( fDet ) * surf_norm - vGrad );
	}
#endif`,B_=`#if NUM_CLIPPING_PLANES > 0
	vec4 plane;
	#ifdef ALPHA_TO_COVERAGE
		float distanceToPlane, distanceGradient;
		float clipOpacity = 1.0;
		#pragma unroll_loop_start
		for ( int i = 0; i < UNION_CLIPPING_PLANES; i ++ ) {
			plane = clippingPlanes[ i ];
			distanceToPlane = - dot( vClipPosition, plane.xyz ) + plane.w;
			distanceGradient = fwidth( distanceToPlane ) / 2.0;
			clipOpacity *= smoothstep( - distanceGradient, distanceGradient, distanceToPlane );
			if ( clipOpacity == 0.0 ) discard;
		}
		#pragma unroll_loop_end
		#if UNION_CLIPPING_PLANES < NUM_CLIPPING_PLANES
			float unionClipOpacity = 1.0;
			#pragma unroll_loop_start
			for ( int i = UNION_CLIPPING_PLANES; i < NUM_CLIPPING_PLANES; i ++ ) {
				plane = clippingPlanes[ i ];
				distanceToPlane = - dot( vClipPosition, plane.xyz ) + plane.w;
				distanceGradient = fwidth( distanceToPlane ) / 2.0;
				unionClipOpacity *= 1.0 - smoothstep( - distanceGradient, distanceGradient, distanceToPlane );
			}
			#pragma unroll_loop_end
			clipOpacity *= 1.0 - unionClipOpacity;
		#endif
		diffuseColor.a *= clipOpacity;
		if ( diffuseColor.a == 0.0 ) discard;
	#else
		#pragma unroll_loop_start
		for ( int i = 0; i < UNION_CLIPPING_PLANES; i ++ ) {
			plane = clippingPlanes[ i ];
			if ( dot( vClipPosition, plane.xyz ) > plane.w ) discard;
		}
		#pragma unroll_loop_end
		#if UNION_CLIPPING_PLANES < NUM_CLIPPING_PLANES
			bool clipped = true;
			#pragma unroll_loop_start
			for ( int i = UNION_CLIPPING_PLANES; i < NUM_CLIPPING_PLANES; i ++ ) {
				plane = clippingPlanes[ i ];
				clipped = ( dot( vClipPosition, plane.xyz ) > plane.w ) && clipped;
			}
			#pragma unroll_loop_end
			if ( clipped ) discard;
		#endif
	#endif
#endif`,F_=`#if NUM_CLIPPING_PLANES > 0
	varying vec3 vClipPosition;
	uniform vec4 clippingPlanes[ NUM_CLIPPING_PLANES ];
#endif`,L_=`#if NUM_CLIPPING_PLANES > 0
	varying vec3 vClipPosition;
#endif`,U_=`#if NUM_CLIPPING_PLANES > 0
	vClipPosition = - mvPosition.xyz;
#endif`,D_=`#if defined( USE_COLOR ) || defined( USE_COLOR_ALPHA )
	diffuseColor *= vColor;
#endif`,P_=`#if defined( USE_COLOR ) || defined( USE_COLOR_ALPHA )
	varying vec4 vColor;
#endif`,G_=`#if defined( USE_COLOR ) || defined( USE_COLOR_ALPHA ) || defined( USE_INSTANCING_COLOR ) || defined( USE_BATCHING_COLOR )
	varying vec4 vColor;
#endif`,V_=`#if defined( USE_COLOR ) || defined( USE_COLOR_ALPHA ) || defined( USE_INSTANCING_COLOR ) || defined( USE_BATCHING_COLOR )
	vColor = vec4( 1.0 );
#endif
#ifdef USE_COLOR_ALPHA
	vColor *= color;
#elif defined( USE_COLOR )
	vColor.rgb *= color;
#endif
#ifdef USE_INSTANCING_COLOR
	vColor.rgb *= instanceColor.rgb;
#endif
#ifdef USE_BATCHING_COLOR
	vColor *= getBatchingColor( getIndirectIndex( gl_DrawID ) );
#endif`,O_=`#define PI 3.141592653589793
#define PI2 6.283185307179586
#define PI_HALF 1.5707963267948966
#define RECIPROCAL_PI 0.3183098861837907
#define RECIPROCAL_PI2 0.15915494309189535
#define EPSILON 1e-6
#ifndef saturate
#define saturate( a ) clamp( a, 0.0, 1.0 )
#endif
#define whiteComplement( a ) ( 1.0 - saturate( a ) )
float pow2( const in float x ) { return x*x; }
vec3 pow2( const in vec3 x ) { return x*x; }
float pow3( const in float x ) { return x*x*x; }
float pow4( const in float x ) { float x2 = x*x; return x2*x2; }
float max3( const in vec3 v ) { return max( max( v.x, v.y ), v.z ); }
float average( const in vec3 v ) { return dot( v, vec3( 0.3333333 ) ); }
highp float rand( const in vec2 uv ) {
	const highp float a = 12.9898, b = 78.233, c = 43758.5453;
	highp float dt = dot( uv.xy, vec2( a,b ) ), sn = mod( dt, PI );
	return fract( sin( sn ) * c );
}
#ifdef HIGH_PRECISION
	float precisionSafeLength( vec3 v ) { return length( v ); }
#else
	float precisionSafeLength( vec3 v ) {
		float maxComponent = max3( abs( v ) );
		return length( v / maxComponent ) * maxComponent;
	}
#endif
struct IncidentLight {
	vec3 color;
	vec3 direction;
	bool visible;
};
struct ReflectedLight {
	vec3 directDiffuse;
	vec3 directSpecular;
	vec3 indirectDiffuse;
	vec3 indirectSpecular;
};
#ifdef USE_ALPHAHASH
	varying vec3 vPosition;
#endif
vec3 transformDirection( in vec3 dir, in mat4 matrix ) {
	return normalize( ( matrix * vec4( dir, 0.0 ) ).xyz );
}
#define inverseTransformDirection transformDirectionByInverseViewMatrix
vec3 transformNormalByInverseViewMatrix( in vec3 normal, in mat4 viewMatrix ) {
	return normalize( ( vec4( normal, 0.0 ) * viewMatrix ).xyz );
}
vec3 transformDirectionByInverseViewMatrix( in vec3 dir, in mat4 viewMatrix ) {
	return normalize( ( vec4( dir, 0.0 ) * viewMatrix ).xyz );
}
bool isPerspectiveMatrix( mat4 m ) {
	return m[ 2 ][ 3 ] == - 1.0;
}
vec2 equirectUv( in vec3 dir ) {
	float u = atan( dir.z, dir.x ) * RECIPROCAL_PI2 + 0.5;
	float v = asin( clamp( dir.y, - 1.0, 1.0 ) ) * RECIPROCAL_PI + 0.5;
	return vec2( u, v );
}
vec3 BRDF_Lambert( const in vec3 diffuseColor ) {
	return RECIPROCAL_PI * diffuseColor;
}
vec3 F_Schlick( const in vec3 f0, const in float f90, const in float dotVH ) {
	float fresnel = exp2( ( - 5.55473 * dotVH - 6.98316 ) * dotVH );
	return f0 * ( 1.0 - fresnel ) + ( f90 * fresnel );
}
float F_Schlick( const in float f0, const in float f90, const in float dotVH ) {
	float fresnel = exp2( ( - 5.55473 * dotVH - 6.98316 ) * dotVH );
	return f0 * ( 1.0 - fresnel ) + ( f90 * fresnel );
} // validated`,k_=`#ifdef ENVMAP_TYPE_CUBE_UV
	#define cubeUV_minMipLevel 4.0
	#define cubeUV_minTileSize 16.0
	float getFace( vec3 direction ) {
		vec3 absDirection = abs( direction );
		float face = - 1.0;
		if ( absDirection.x > absDirection.z ) {
			if ( absDirection.x > absDirection.y )
				face = direction.x > 0.0 ? 0.0 : 3.0;
			else
				face = direction.y > 0.0 ? 1.0 : 4.0;
		} else {
			if ( absDirection.z > absDirection.y )
				face = direction.z > 0.0 ? 2.0 : 5.0;
			else
				face = direction.y > 0.0 ? 1.0 : 4.0;
		}
		return face;
	}
	vec2 getUV( vec3 direction, float face ) {
		vec2 uv;
		if ( face == 0.0 ) {
			uv = vec2( direction.z, direction.y ) / abs( direction.x );
		} else if ( face == 1.0 ) {
			uv = vec2( - direction.x, - direction.z ) / abs( direction.y );
		} else if ( face == 2.0 ) {
			uv = vec2( - direction.x, direction.y ) / abs( direction.z );
		} else if ( face == 3.0 ) {
			uv = vec2( - direction.z, direction.y ) / abs( direction.x );
		} else if ( face == 4.0 ) {
			uv = vec2( - direction.x, direction.z ) / abs( direction.y );
		} else {
			uv = vec2( direction.x, direction.y ) / abs( direction.z );
		}
		return 0.5 * ( uv + 1.0 );
	}
	vec3 bilinearCubeUV( sampler2D envMap, vec3 direction, float mipInt ) {
		float face = getFace( direction );
		float filterInt = max( cubeUV_minMipLevel - mipInt, 0.0 );
		mipInt = max( mipInt, cubeUV_minMipLevel );
		float faceSize = exp2( mipInt );
		highp vec2 uv = getUV( direction, face ) * ( faceSize - 2.0 ) + 1.0;
		if ( face > 2.0 ) {
			uv.y += faceSize;
			face -= 3.0;
		}
		uv.x += face * faceSize;
		uv.x += filterInt * 3.0 * cubeUV_minTileSize;
		uv.y += 4.0 * ( exp2( CUBEUV_MAX_MIP ) - faceSize );
		uv.x *= CUBEUV_TEXEL_WIDTH;
		uv.y *= CUBEUV_TEXEL_HEIGHT;
		#ifdef texture2DGradEXT
			return texture2DGradEXT( envMap, uv, vec2( 0.0 ), vec2( 0.0 ) ).rgb;
		#else
			return texture2D( envMap, uv ).rgb;
		#endif
	}
	#define cubeUV_r0 1.0
	#define cubeUV_m0 - 2.0
	#define cubeUV_r1 0.8
	#define cubeUV_m1 - 1.0
	#define cubeUV_r4 0.4
	#define cubeUV_m4 2.0
	#define cubeUV_r5 0.305
	#define cubeUV_m5 3.0
	#define cubeUV_r6 0.21
	#define cubeUV_m6 4.0
	float roughnessToMip( float roughness ) {
		float mip = 0.0;
		if ( roughness >= cubeUV_r1 ) {
			mip = ( cubeUV_r0 - roughness ) * ( cubeUV_m1 - cubeUV_m0 ) / ( cubeUV_r0 - cubeUV_r1 ) + cubeUV_m0;
		} else if ( roughness >= cubeUV_r4 ) {
			mip = ( cubeUV_r1 - roughness ) * ( cubeUV_m4 - cubeUV_m1 ) / ( cubeUV_r1 - cubeUV_r4 ) + cubeUV_m1;
		} else if ( roughness >= cubeUV_r5 ) {
			mip = ( cubeUV_r4 - roughness ) * ( cubeUV_m5 - cubeUV_m4 ) / ( cubeUV_r4 - cubeUV_r5 ) + cubeUV_m4;
		} else if ( roughness >= cubeUV_r6 ) {
			mip = ( cubeUV_r5 - roughness ) * ( cubeUV_m6 - cubeUV_m5 ) / ( cubeUV_r5 - cubeUV_r6 ) + cubeUV_m5;
		} else {
			mip = - 2.0 * log2( 1.16 * roughness );		}
		return mip;
	}
	vec4 textureCubeUV( sampler2D envMap, vec3 sampleDir, float roughness ) {
		float mip = clamp( roughnessToMip( roughness ), cubeUV_m0, CUBEUV_MAX_MIP );
		float mipF = fract( mip );
		float mipInt = floor( mip );
		vec3 color0 = bilinearCubeUV( envMap, sampleDir, mipInt );
		if ( mipF == 0.0 ) {
			return vec4( color0, 1.0 );
		} else {
			vec3 color1 = bilinearCubeUV( envMap, sampleDir, mipInt + 1.0 );
			return vec4( mix( color0, color1, mipF ), 1.0 );
		}
	}
#endif`,z_=`vec3 transformedNormal = objectNormal;
#ifdef USE_TANGENT
	vec3 transformedTangent = objectTangent;
#endif
#ifdef USE_BATCHING
	mat3 bm = mat3( batchingMatrix );
	transformedNormal /= vec3( dot( bm[ 0 ], bm[ 0 ] ), dot( bm[ 1 ], bm[ 1 ] ), dot( bm[ 2 ], bm[ 2 ] ) );
	transformedNormal = bm * transformedNormal;
	#ifdef USE_TANGENT
		transformedTangent = bm * transformedTangent;
	#endif
#endif
#ifdef USE_INSTANCING
	mat3 im = mat3( instanceMatrix );
	transformedNormal /= vec3( dot( im[ 0 ], im[ 0 ] ), dot( im[ 1 ], im[ 1 ] ), dot( im[ 2 ], im[ 2 ] ) );
	transformedNormal = im * transformedNormal;
	#ifdef USE_TANGENT
		transformedTangent = im * transformedTangent;
	#endif
#endif
transformedNormal = normalMatrix * transformedNormal;
#ifdef FLIP_SIDED
	transformedNormal = - transformedNormal;
#endif
#ifdef USE_TANGENT
	transformedTangent = ( modelViewMatrix * vec4( transformedTangent, 0.0 ) ).xyz;
#endif`,W_=`#ifdef USE_DISPLACEMENTMAP
	uniform sampler2D displacementMap;
	uniform float displacementScale;
	uniform float displacementBias;
#endif`,H_=`#ifdef USE_DISPLACEMENTMAP
	transformed += normalize( objectNormal ) * ( texture2D( displacementMap, vDisplacementMapUv ).x * displacementScale + displacementBias );
#endif`,Y_=`#ifdef USE_EMISSIVEMAP
	vec4 emissiveColor = texture2D( emissiveMap, vEmissiveMapUv );
	#ifdef DECODE_VIDEO_TEXTURE_EMISSIVE
		emissiveColor = sRGBTransferEOTF( emissiveColor );
	#endif
	totalEmissiveRadiance *= emissiveColor.rgb;
#endif`,Z_=`#ifdef USE_EMISSIVEMAP
	uniform sampler2D emissiveMap;
#endif`,X_="gl_FragColor = linearToOutputTexel( gl_FragColor );",K_=`vec4 LinearTransferOETF( in vec4 value ) {
	return value;
}
vec4 sRGBTransferEOTF( in vec4 value ) {
	return vec4( mix( pow( value.rgb * 0.9478672986 + vec3( 0.0521327014 ), vec3( 2.4 ) ), value.rgb * 0.0773993808, vec3( lessThanEqual( value.rgb, vec3( 0.04045 ) ) ) ), value.a );
}
vec4 sRGBTransferOETF( in vec4 value ) {
	return vec4( mix( pow( value.rgb, vec3( 0.41666 ) ) * 1.055 - vec3( 0.055 ), value.rgb * 12.92, vec3( lessThanEqual( value.rgb, vec3( 0.0031308 ) ) ) ), value.a );
}`,$_=`#ifdef USE_ENVMAP
	#ifdef ENV_WORLDPOS
		vec3 cameraToFrag;
		if ( isOrthographic ) {
			cameraToFrag = normalize( vec3( - viewMatrix[ 0 ][ 2 ], - viewMatrix[ 1 ][ 2 ], - viewMatrix[ 2 ][ 2 ] ) );
		} else {
			cameraToFrag = normalize( vWorldPosition - cameraPosition );
		}
		vec3 worldNormal = transformNormalByInverseViewMatrix( normal, viewMatrix );
		#ifdef ENVMAP_MODE_REFLECTION
			vec3 reflectVec = reflect( cameraToFrag, worldNormal );
		#else
			vec3 reflectVec = refract( cameraToFrag, worldNormal, refractionRatio );
		#endif
	#else
		vec3 reflectVec = vReflect;
	#endif
	#ifdef ENVMAP_TYPE_CUBE
		vec4 envColor = textureCube( envMap, envMapRotation * reflectVec );
		#ifdef ENVMAP_BLENDING_MULTIPLY
			outgoingLight = mix( outgoingLight, outgoingLight * envColor.xyz, specularStrength * reflectivity );
		#elif defined( ENVMAP_BLENDING_MIX )
			outgoingLight = mix( outgoingLight, envColor.xyz, specularStrength * reflectivity );
		#elif defined( ENVMAP_BLENDING_ADD )
			outgoingLight += envColor.xyz * specularStrength * reflectivity;
		#endif
	#endif
#endif`,J_=`#ifdef USE_ENVMAP
	uniform float envMapIntensity;
	uniform mat3 envMapRotation;
	#ifdef ENVMAP_TYPE_CUBE
		uniform samplerCube envMap;
	#else
		uniform sampler2D envMap;
	#endif
#endif`,q_=`#ifdef USE_ENVMAP
	uniform float reflectivity;
	#if defined( USE_BUMPMAP ) || defined( USE_NORMALMAP ) || defined( PHONG ) || defined( LAMBERT )
		#define ENV_WORLDPOS
	#endif
	#ifdef ENV_WORLDPOS
		varying vec3 vWorldPosition;
		uniform float refractionRatio;
	#else
		varying vec3 vReflect;
	#endif
#endif`,Q_=`#ifdef USE_ENVMAP
	#if defined( USE_BUMPMAP ) || defined( USE_NORMALMAP ) || defined( PHONG ) || defined( LAMBERT )
		#define ENV_WORLDPOS
	#endif
	#ifdef ENV_WORLDPOS
		
		varying vec3 vWorldPosition;
	#else
		varying vec3 vReflect;
		uniform float refractionRatio;
	#endif
#endif`,j_=`#ifdef USE_ENVMAP
	#ifdef ENV_WORLDPOS
		vWorldPosition = worldPosition.xyz;
	#else
		vec3 cameraToVertex;
		if ( isOrthographic ) {
			cameraToVertex = normalize( vec3( - viewMatrix[ 0 ][ 2 ], - viewMatrix[ 1 ][ 2 ], - viewMatrix[ 2 ][ 2 ] ) );
		} else {
			cameraToVertex = normalize( worldPosition.xyz - cameraPosition );
		}
		vec3 worldNormal = transformNormalByInverseViewMatrix( transformedNormal, viewMatrix );
		#ifdef ENVMAP_MODE_REFLECTION
			vReflect = reflect( cameraToVertex, worldNormal );
		#else
			vReflect = refract( cameraToVertex, worldNormal, refractionRatio );
		#endif
	#endif
#endif`,eb=`#ifdef USE_FOG
	vFogDepth = - mvPosition.z;
#endif`,tb=`#ifdef USE_FOG
	varying float vFogDepth;
#endif`,nb=`#ifdef USE_FOG
	#ifdef FOG_EXP2
		float fogFactor = 1.0 - exp( - fogDensity * fogDensity * vFogDepth * vFogDepth );
	#else
		float fogFactor = smoothstep( fogNear, fogFar, vFogDepth );
	#endif
	gl_FragColor.rgb = mix( gl_FragColor.rgb, fogColor, fogFactor );
#endif`,ib=`#ifdef USE_FOG
	uniform vec3 fogColor;
	varying float vFogDepth;
	#ifdef FOG_EXP2
		uniform float fogDensity;
	#else
		uniform float fogNear;
		uniform float fogFar;
	#endif
#endif`,sb=`#ifdef USE_GRADIENTMAP
	uniform sampler2D gradientMap;
#endif
vec3 getGradientIrradiance( vec3 normal, vec3 lightDirection ) {
	float dotNL = dot( normal, lightDirection );
	vec2 coord = vec2( dotNL * 0.5 + 0.5, 0.0 );
	#ifdef USE_GRADIENTMAP
		return vec3( texture2D( gradientMap, coord ).r );
	#else
		vec2 fw = fwidth( coord ) * 0.5;
		return mix( vec3( 0.7 ), vec3( 1.0 ), smoothstep( 0.7 - fw.x, 0.7 + fw.x, coord.x ) );
	#endif
}`,rb=`#ifdef USE_LIGHTMAP
	uniform sampler2D lightMap;
	uniform float lightMapIntensity;
#endif`,ob=`LambertMaterial material;
material.diffuseColor = diffuseColor.rgb;
material.specularStrength = specularStrength;`,ab=`varying vec3 vViewPosition;
struct LambertMaterial {
	vec3 diffuseColor;
	float specularStrength;
};
void RE_Direct_Lambert( const in IncidentLight directLight, const in vec3 geometryPosition, const in vec3 geometryNormal, const in vec3 geometryViewDir, const in vec3 geometryClearcoatNormal, const in LambertMaterial material, inout ReflectedLight reflectedLight ) {
	float dotNL = saturate( dot( geometryNormal, directLight.direction ) );
	vec3 irradiance = dotNL * directLight.color;
	reflectedLight.directDiffuse += irradiance * BRDF_Lambert( material.diffuseColor );
}
void RE_IndirectDiffuse_Lambert( const in vec3 irradiance, const in vec3 geometryPosition, const in vec3 geometryNormal, const in vec3 geometryViewDir, const in vec3 geometryClearcoatNormal, const in LambertMaterial material, inout ReflectedLight reflectedLight ) {
	reflectedLight.indirectDiffuse += irradiance * BRDF_Lambert( material.diffuseColor );
}
#define RE_Direct				RE_Direct_Lambert
#define RE_IndirectDiffuse		RE_IndirectDiffuse_Lambert`,lb=`uniform bool receiveShadow;
uniform vec3 ambientLightColor;
#if defined( USE_LIGHT_PROBES )
	uniform vec3 lightProbe[ 9 ];
#endif
vec3 shGetIrradianceAt( in vec3 normal, in vec3 shCoefficients[ 9 ] ) {
	float x = normal.x, y = normal.y, z = normal.z;
	vec3 result = shCoefficients[ 0 ] * 0.886227;
	result += shCoefficients[ 1 ] * 2.0 * 0.511664 * y;
	result += shCoefficients[ 2 ] * 2.0 * 0.511664 * z;
	result += shCoefficients[ 3 ] * 2.0 * 0.511664 * x;
	result += shCoefficients[ 4 ] * 2.0 * 0.429043 * x * y;
	result += shCoefficients[ 5 ] * 2.0 * 0.429043 * y * z;
	result += shCoefficients[ 6 ] * ( 0.743125 * z * z - 0.247708 );
	result += shCoefficients[ 7 ] * 2.0 * 0.429043 * x * z;
	result += shCoefficients[ 8 ] * 0.429043 * ( x * x - y * y );
	return result;
}
vec3 getLightProbeIrradiance( const in vec3 lightProbe[ 9 ], const in vec3 normal ) {
	vec3 worldNormal = transformNormalByInverseViewMatrix( normal, viewMatrix );
	vec3 irradiance = shGetIrradianceAt( worldNormal, lightProbe );
	return irradiance;
}
vec3 getAmbientLightIrradiance( const in vec3 ambientLightColor ) {
	vec3 irradiance = ambientLightColor;
	return irradiance;
}
float getDistanceAttenuation( const in float lightDistance, const in float cutoffDistance, const in float decayExponent ) {
	float distanceFalloff = 1.0 / max( pow( lightDistance, decayExponent ), 0.01 );
	if ( cutoffDistance > 0.0 ) {
		distanceFalloff *= pow2( saturate( 1.0 - pow4( lightDistance / cutoffDistance ) ) );
	}
	return distanceFalloff;
}
float getSpotAttenuation( const in float coneCosine, const in float penumbraCosine, const in float angleCosine ) {
	return smoothstep( coneCosine, penumbraCosine, angleCosine );
}
#if NUM_SUN_LIGHTS > 0
	struct SunLight {
		vec3 direction;
		vec3 color;
	};
	uniform SunLight sunLights[ NUM_SUN_LIGHTS ];
	void getSunLightInfo( const in SunLight sunLight, out IncidentLight light ) {
		light.color = sunLight.color;
		light.direction = sunLight.direction;
		light.visible = true;
	}
#endif
#if NUM_DIR_LIGHTS > 0
	struct DirectionalLight {
		vec3 direction;
		vec3 color;
	};
	uniform DirectionalLight directionalLights[ NUM_DIR_LIGHTS ];
	void getDirectionalLightInfo( const in DirectionalLight directionalLight, out IncidentLight light ) {
		light.color = directionalLight.color;
		light.direction = directionalLight.direction;
		light.visible = true;
	}
#endif
#if NUM_POINT_LIGHTS > 0
	struct PointLight {
		vec3 position;
		vec3 color;
		float distance;
		float decay;
	};
	uniform PointLight pointLights[ NUM_POINT_LIGHTS ];
	void getPointLightInfo( const in PointLight pointLight, const in vec3 geometryPosition, out IncidentLight light ) {
		vec3 lVector = pointLight.position - geometryPosition;
		light.direction = normalize( lVector );
		float lightDistance = length( lVector );
		light.color = pointLight.color;
		light.color *= getDistanceAttenuation( lightDistance, pointLight.distance, pointLight.decay );
		light.visible = ( light.color != vec3( 0.0 ) );
	}
#endif
#if NUM_SPOT_LIGHTS > 0
	struct SpotLight {
		vec3 position;
		vec3 direction;
		vec3 color;
		float distance;
		float decay;
		float coneCos;
		float penumbraCos;
	};
	uniform SpotLight spotLights[ NUM_SPOT_LIGHTS ];
	void getSpotLightInfo( const in SpotLight spotLight, const in vec3 geometryPosition, out IncidentLight light ) {
		vec3 lVector = spotLight.position - geometryPosition;
		light.direction = normalize( lVector );
		float angleCos = dot( light.direction, spotLight.direction );
		float spotAttenuation = getSpotAttenuation( spotLight.coneCos, spotLight.penumbraCos, angleCos );
		if ( spotAttenuation > 0.0 ) {
			float lightDistance = length( lVector );
			light.color = spotLight.color * spotAttenuation;
			light.color *= getDistanceAttenuation( lightDistance, spotLight.distance, spotLight.decay );
			light.visible = ( light.color != vec3( 0.0 ) );
		} else {
			light.color = vec3( 0.0 );
			light.visible = false;
		}
	}
#endif
#if NUM_RECT_AREA_LIGHTS > 0
	struct RectAreaLight {
		vec3 color;
		vec3 position;
		vec3 halfWidth;
		vec3 halfHeight;
	};
	uniform sampler2D ltc_1;	uniform sampler2D ltc_2;
	uniform RectAreaLight rectAreaLights[ NUM_RECT_AREA_LIGHTS ];
#endif
#if NUM_HEMI_LIGHTS > 0
	struct HemisphereLight {
		vec3 direction;
		vec3 skyColor;
		vec3 groundColor;
	};
	uniform HemisphereLight hemisphereLights[ NUM_HEMI_LIGHTS ];
	vec3 getHemisphereLightIrradiance( const in HemisphereLight hemiLight, const in vec3 normal ) {
		float dotNL = dot( normal, hemiLight.direction );
		float hemiDiffuseWeight = 0.5 * dotNL + 0.5;
		vec3 irradiance = mix( hemiLight.groundColor, hemiLight.skyColor, hemiDiffuseWeight );
		return irradiance;
	}
#endif
#include <lightprobes_pars_fragment>`,cb=`#ifdef USE_ENVMAP
	vec3 getIBLIrradiance( const in vec3 normal ) {
		#ifdef ENVMAP_TYPE_CUBE_UV
			vec3 worldNormal = transformNormalByInverseViewMatrix( normal, viewMatrix );
			vec4 envMapColor = textureCubeUV( envMap, envMapRotation * worldNormal, 1.0 );
			return PI * envMapColor.rgb * envMapIntensity;
		#else
			return vec3( 0.0 );
		#endif
	}
	vec3 getIBLRadiance( const in vec3 viewDir, const in vec3 normal, const in float roughness ) {
		#ifdef ENVMAP_TYPE_CUBE_UV
			vec3 reflectVec = reflect( - viewDir, normal );
			reflectVec = normalize( mix( reflectVec, normal, pow4( roughness ) ) );
			reflectVec = transformDirectionByInverseViewMatrix( reflectVec, viewMatrix );
			vec4 envMapColor = textureCubeUV( envMap, envMapRotation * reflectVec, roughness );
			return envMapColor.rgb * envMapIntensity;
		#else
			return vec3( 0.0 );
		#endif
	}
	#ifdef USE_RETROREFLECTION
		vec3 getIBLRetroRadiance( const in vec3 viewDir, const in vec3 normal, const in float roughness ) {
			#ifdef ENVMAP_TYPE_CUBE_UV
				vec3 retroVec = normalize( mix( viewDir, normal, pow4( roughness ) ) );
				retroVec = transformDirectionByInverseViewMatrix( retroVec, viewMatrix );
				vec4 envMapColor = textureCubeUV( envMap, envMapRotation * retroVec, roughness );
				return envMapColor.rgb * envMapIntensity;
			#else
				return vec3( 0.0 );
			#endif
		}
	#endif
	#ifdef USE_ANISOTROPY
		vec3 getIBLAnisotropyRadiance( const in vec3 viewDir, const in vec3 normal, const in float roughness, const in vec3 bitangent, const in float anisotropy ) {
			#ifdef ENVMAP_TYPE_CUBE_UV
				vec3 bentNormal = cross( bitangent, viewDir );
				bentNormal = normalize( cross( bentNormal, bitangent ) );
				bentNormal = normalize( mix( bentNormal, normal, pow2( pow2( 1.0 - anisotropy * ( 1.0 - roughness ) ) ) ) );
				return getIBLRadiance( viewDir, bentNormal, roughness );
			#else
				return vec3( 0.0 );
			#endif
		}
		#ifdef USE_RETROREFLECTION
			vec3 getIBLAnisotropyRetroRadiance( const in vec3 viewDir, const in vec3 normal, const in float roughness, const in vec3 bitangent, const in float anisotropy ) {
				#ifdef ENVMAP_TYPE_CUBE_UV
					vec3 bentNormal = cross( bitangent, viewDir );
					bentNormal = normalize( cross( bentNormal, bitangent ) );
					bentNormal = normalize( mix( bentNormal, normal, pow2( pow2( 1.0 - anisotropy * ( 1.0 - roughness ) ) ) ) );
					return getIBLRetroRadiance( viewDir, bentNormal, roughness );
				#else
					return vec3( 0.0 );
				#endif
			}
		#endif
	#endif
#endif`,ub=`ToonMaterial material;
material.diffuseColor = diffuseColor.rgb;`,hb=`varying vec3 vViewPosition;
struct ToonMaterial {
	vec3 diffuseColor;
};
void RE_Direct_Toon( const in IncidentLight directLight, const in vec3 geometryPosition, const in vec3 geometryNormal, const in vec3 geometryViewDir, const in vec3 geometryClearcoatNormal, const in ToonMaterial material, inout ReflectedLight reflectedLight ) {
	vec3 irradiance = getGradientIrradiance( geometryNormal, directLight.direction ) * directLight.color;
	reflectedLight.directDiffuse += irradiance * BRDF_Lambert( material.diffuseColor );
}
void RE_IndirectDiffuse_Toon( const in vec3 irradiance, const in vec3 geometryPosition, const in vec3 geometryNormal, const in vec3 geometryViewDir, const in vec3 geometryClearcoatNormal, const in ToonMaterial material, inout ReflectedLight reflectedLight ) {
	reflectedLight.indirectDiffuse += irradiance * BRDF_Lambert( material.diffuseColor );
}
#define RE_Direct				RE_Direct_Toon
#define RE_IndirectDiffuse		RE_IndirectDiffuse_Toon`,db=`BlinnPhongMaterial material;
material.diffuseColor = diffuseColor.rgb;
material.specularColor = specular;
material.specularShininess = shininess;
material.specularStrength = specularStrength;`,fb=`varying vec3 vViewPosition;
struct BlinnPhongMaterial {
	vec3 diffuseColor;
	vec3 specularColor;
	float specularShininess;
	float specularStrength;
};
void RE_Direct_BlinnPhong( const in IncidentLight directLight, const in vec3 geometryPosition, const in vec3 geometryNormal, const in vec3 geometryViewDir, const in vec3 geometryClearcoatNormal, const in BlinnPhongMaterial material, inout ReflectedLight reflectedLight ) {
	float dotNL = saturate( dot( geometryNormal, directLight.direction ) );
	vec3 irradiance = dotNL * directLight.color;
	reflectedLight.directDiffuse += irradiance * BRDF_Lambert( material.diffuseColor );
	reflectedLight.directSpecular += irradiance * BRDF_BlinnPhong( directLight.direction, geometryViewDir, geometryNormal, material.specularColor, material.specularShininess ) * material.specularStrength;
}
void RE_IndirectDiffuse_BlinnPhong( const in vec3 irradiance, const in vec3 geometryPosition, const in vec3 geometryNormal, const in vec3 geometryViewDir, const in vec3 geometryClearcoatNormal, const in BlinnPhongMaterial material, inout ReflectedLight reflectedLight ) {
	reflectedLight.indirectDiffuse += irradiance * BRDF_Lambert( material.diffuseColor );
}
#define RE_Direct				RE_Direct_BlinnPhong
#define RE_IndirectDiffuse		RE_IndirectDiffuse_BlinnPhong`,pb=`PhysicalMaterial material;
material.diffuseColor = diffuseColor.rgb;
material.diffuseContribution = diffuseColor.rgb * ( 1.0 - metalnessFactor );
material.metalness = metalnessFactor;
vec3 dxy = max( abs( dFdx( nonPerturbedNormal ) ), abs( dFdy( nonPerturbedNormal ) ) );
float geometryRoughness = max( max( dxy.x, dxy.y ), dxy.z );
material.roughness = max( roughnessFactor, 0.0525 );material.roughness += geometryRoughness;
material.roughness = min( material.roughness, 1.0 );
#ifdef IOR
	material.ior = ior;
	#ifdef USE_SPECULAR
		float specularIntensityFactor = specularIntensity;
		vec3 specularColorFactor = specularColor;
		#ifdef USE_SPECULAR_COLORMAP
			specularColorFactor *= texture2D( specularColorMap, vSpecularColorMapUv ).rgb;
		#endif
		#ifdef USE_SPECULAR_INTENSITYMAP
			specularIntensityFactor *= texture2D( specularIntensityMap, vSpecularIntensityMapUv ).a;
		#endif
		material.specularF90 = mix( specularIntensityFactor, 1.0, metalnessFactor );
	#else
		float specularIntensityFactor = 1.0;
		vec3 specularColorFactor = vec3( 1.0 );
		material.specularF90 = 1.0;
	#endif
	material.specularColor = min( pow2( ( material.ior - 1.0 ) / ( material.ior + 1.0 ) ) * specularColorFactor, vec3( 1.0 ) ) * specularIntensityFactor;
	material.specularColorBlended = mix( material.specularColor, diffuseColor.rgb, metalnessFactor );
#else
	material.specularColor = vec3( 0.04 );
	material.specularColorBlended = mix( material.specularColor, diffuseColor.rgb, metalnessFactor );
	material.specularF90 = 1.0;
#endif
#ifdef USE_CLEARCOAT
	material.clearcoat = clearcoat;
	material.clearcoatRoughness = clearcoatRoughness;
	material.clearcoatF0 = vec3( 0.04 );
	material.clearcoatF90 = 1.0;
	#ifdef USE_CLEARCOATMAP
		material.clearcoat *= texture2D( clearcoatMap, vClearcoatMapUv ).x;
	#endif
	#ifdef USE_CLEARCOAT_ROUGHNESSMAP
		material.clearcoatRoughness *= texture2D( clearcoatRoughnessMap, vClearcoatRoughnessMapUv ).y;
	#endif
	material.clearcoat = saturate( material.clearcoat );	material.clearcoatRoughness = max( material.clearcoatRoughness, 0.0525 );
	material.clearcoatRoughness += geometryRoughness;
	material.clearcoatRoughness = min( material.clearcoatRoughness, 1.0 );
#endif
#ifdef USE_DISPERSION
	material.dispersion = dispersion;
#endif
#ifdef USE_RETROREFLECTION
	material.retroreflectivity = retroreflectivity;
#endif
#ifdef USE_IRIDESCENCE
	material.iridescence = iridescence;
	material.iridescenceIOR = iridescenceIOR;
	#ifdef USE_IRIDESCENCEMAP
		material.iridescence *= texture2D( iridescenceMap, vIridescenceMapUv ).r;
	#endif
	#ifdef USE_IRIDESCENCE_THICKNESSMAP
		material.iridescenceThickness = (iridescenceThicknessMaximum - iridescenceThicknessMinimum) * texture2D( iridescenceThicknessMap, vIridescenceThicknessMapUv ).g + iridescenceThicknessMinimum;
	#else
		material.iridescenceThickness = iridescenceThicknessMaximum;
	#endif
#endif
#ifdef USE_SHEEN
	material.sheenColor = sheenColor;
	#ifdef USE_SHEEN_COLORMAP
		material.sheenColor *= texture2D( sheenColorMap, vSheenColorMapUv ).rgb;
	#endif
	material.sheenRoughness = clamp( sheenRoughness, 0.0001, 1.0 );
	#ifdef USE_SHEEN_ROUGHNESSMAP
		material.sheenRoughness *= texture2D( sheenRoughnessMap, vSheenRoughnessMapUv ).a;
	#endif
#endif
#ifdef USE_ANISOTROPY
	#ifdef USE_ANISOTROPYMAP
		mat2 anisotropyMat = mat2( anisotropyVector.x, anisotropyVector.y, - anisotropyVector.y, anisotropyVector.x );
		vec3 anisotropyPolar = texture2D( anisotropyMap, vAnisotropyMapUv ).rgb;
		vec2 anisotropyV = anisotropyMat * normalize( 2.0 * anisotropyPolar.rg - vec2( 1.0 ) ) * anisotropyPolar.b;
	#else
		vec2 anisotropyV = anisotropyVector;
	#endif
	material.anisotropy = length( anisotropyV );
	if( material.anisotropy == 0.0 ) {
		anisotropyV = vec2( 1.0, 0.0 );
	} else {
		anisotropyV /= material.anisotropy;
		material.anisotropy = saturate( material.anisotropy );
	}
	material.alphaT = mix( pow2( material.roughness ), 1.0, pow2( material.anisotropy ) );
	material.anisotropyT = tbn[ 0 ] * anisotropyV.x + tbn[ 1 ] * anisotropyV.y;
	material.anisotropyB = tbn[ 1 ] * anisotropyV.x - tbn[ 0 ] * anisotropyV.y;
#endif`,mb=`uniform sampler2D dfgLUT;
struct PhysicalMaterial {
	vec3 diffuseColor;
	vec3 diffuseContribution;
	vec3 specularColor;
	vec3 specularColorBlended;
	float roughness;
	float metalness;
	float specularF90;
	float dispersion;
	vec2 dfg;
	vec3 multiScatteringCompensation;
	#ifdef USE_RETROREFLECTION
		float retroreflectivity;
	#endif
	#ifdef USE_CLEARCOAT
		float clearcoat;
		float clearcoatRoughness;
		vec3 clearcoatF0;
		float clearcoatF90;
	#endif
	#ifdef USE_IRIDESCENCE
		float iridescence;
		float iridescenceIOR;
		float iridescenceThickness;
		vec3 iridescenceFresnel;
		vec3 iridescenceF0Dielectric;
		vec3 iridescenceF0Metallic;
	#endif
	#ifdef USE_SHEEN
		vec3 sheenColor;
		float sheenRoughness;
	#endif
	#ifdef IOR
		float ior;
	#endif
	#ifdef USE_TRANSMISSION
		float transmission;
		float transmissionAlpha;
		float thickness;
		float attenuationDistance;
		vec3 attenuationColor;
	#endif
	#ifdef USE_ANISOTROPY
		float anisotropy;
		float alphaT;
		vec3 anisotropyT;
		vec3 anisotropyB;
	#endif
};
vec3 clearcoatSpecularDirect = vec3( 0.0 );
vec3 clearcoatSpecularIndirect = vec3( 0.0 );
vec3 sheenSpecularDirect = vec3( 0.0 );
vec3 sheenSpecularIndirect = vec3(0.0 );
vec3 Schlick_to_F0( const in vec3 f, const in float f90, const in float dotVH ) {
    float x = clamp( 1.0 - dotVH, 0.0, 1.0 );
    float x2 = x * x;
    float x5 = clamp( x * x2 * x2, 0.0, 0.9999 );
    return ( f - vec3( f90 ) * x5 ) / ( 1.0 - x5 );
}
float V_GGX_SmithCorrelated( const in float alpha, const in float dotNL, const in float dotNV ) {
	float a2 = pow2( alpha );
	float gv = dotNL * sqrt( a2 + ( 1.0 - a2 ) * pow2( dotNV ) );
	float gl = dotNV * sqrt( a2 + ( 1.0 - a2 ) * pow2( dotNL ) );
	return 0.5 / max( gv + gl, EPSILON );
}
float D_GGX( const in float alpha, const in float dotNH ) {
	float a2 = pow2( alpha );
	float denom = pow2( dotNH ) * ( a2 - 1.0 ) + 1.0;
	return RECIPROCAL_PI * a2 / pow2( denom );
}
#ifdef USE_ANISOTROPY
	float V_GGX_SmithCorrelated_Anisotropic( const in float alphaT, const in float alphaB, const in float dotTV, const in float dotBV, const in float dotTL, const in float dotBL, const in float dotNV, const in float dotNL ) {
		float gv = dotNL * length( vec3( alphaT * dotTV, alphaB * dotBV, dotNV ) );
		float gl = dotNV * length( vec3( alphaT * dotTL, alphaB * dotBL, dotNL ) );
		return 0.5 / max( gv + gl, EPSILON );
	}
	float D_GGX_Anisotropic( const in float alphaT, const in float alphaB, const in float dotNH, const in float dotTH, const in float dotBH ) {
		float a2 = alphaT * alphaB;
		highp vec3 v = vec3( alphaB * dotTH, alphaT * dotBH, a2 * dotNH );
		highp float v2 = dot( v, v );
		float w2 = a2 / v2;
		return RECIPROCAL_PI * a2 * pow2 ( w2 );
	}
#endif
#ifdef USE_CLEARCOAT
	vec3 BRDF_GGX_Clearcoat( const in vec3 lightDir, const in vec3 viewDir, const in vec3 normal, const in PhysicalMaterial material) {
		vec3 f0 = material.clearcoatF0;
		float f90 = material.clearcoatF90;
		float roughness = material.clearcoatRoughness;
		float alpha = pow2( roughness );
		vec3 halfDir = normalize( lightDir + viewDir );
		float dotNL = saturate( dot( normal, lightDir ) );
		float dotNV = saturate( dot( normal, viewDir ) );
		float dotNH = saturate( dot( normal, halfDir ) );
		float dotVH = saturate( dot( viewDir, halfDir ) );
		vec3 F = F_Schlick( f0, f90, dotVH );
		float V = V_GGX_SmithCorrelated( alpha, dotNL, dotNV );
		float D = D_GGX( alpha, dotNH );
		return F * ( V * D );
	}
#endif
vec3 BRDF_GGX( const in vec3 lightDir, const in vec3 viewDir, const in vec3 normal, const in PhysicalMaterial material ) {
	vec3 f0 = material.specularColorBlended;
	float f90 = material.specularF90;
	float roughness = material.roughness;
	float alpha = pow2( roughness );
	vec3 halfDir = normalize( lightDir + viewDir );
	float dotNL = saturate( dot( normal, lightDir ) );
	float dotNV = saturate( dot( normal, viewDir ) );
	float dotNH = saturate( dot( normal, halfDir ) );
	float dotVH = saturate( dot( viewDir, halfDir ) );
	vec3 F = F_Schlick( f0, f90, dotVH );
	#ifdef USE_IRIDESCENCE
		F = mix( F, material.iridescenceFresnel, material.iridescence );
	#endif
	#ifdef USE_ANISOTROPY
		float dotTL = dot( material.anisotropyT, lightDir );
		float dotTV = dot( material.anisotropyT, viewDir );
		float dotTH = dot( material.anisotropyT, halfDir );
		float dotBL = dot( material.anisotropyB, lightDir );
		float dotBV = dot( material.anisotropyB, viewDir );
		float dotBH = dot( material.anisotropyB, halfDir );
		float V = V_GGX_SmithCorrelated_Anisotropic( material.alphaT, alpha, dotTV, dotBV, dotTL, dotBL, dotNV, dotNL );
		float D = D_GGX_Anisotropic( material.alphaT, alpha, dotNH, dotTH, dotBH );
	#else
		float V = V_GGX_SmithCorrelated( alpha, dotNL, dotNV );
		float D = D_GGX( alpha, dotNH );
	#endif
	return F * ( V * D );
}
vec2 LTC_Uv( const in vec3 N, const in vec3 V, const in float roughness ) {
	const float LUT_SIZE = 64.0;
	const float LUT_SCALE = ( LUT_SIZE - 1.0 ) / LUT_SIZE;
	const float LUT_BIAS = 0.5 / LUT_SIZE;
	float dotNV = saturate( dot( N, V ) );
	vec2 uv = vec2( roughness, sqrt( 1.0 - dotNV ) );
	uv = uv * LUT_SCALE + LUT_BIAS;
	return uv;
}
float LTC_ClippedSphereFormFactor( const in vec3 f ) {
	float l = length( f );
	return max( ( l * l + f.z ) / ( l + 1.0 ), 0.0 );
}
vec3 LTC_EdgeVectorFormFactor( const in vec3 v1, const in vec3 v2 ) {
	float x = dot( v1, v2 );
	float y = abs( x );
	float a = 0.8543985 + ( 0.4965155 + 0.0145206 * y ) * y;
	float b = 3.4175940 + ( 4.1616724 + y ) * y;
	float v = a / b;
	float theta_sintheta = ( x > 0.0 ) ? v : 0.5 * inversesqrt( max( 1.0 - x * x, 1e-7 ) ) - v;
	return cross( v1, v2 ) * theta_sintheta;
}
vec3 LTC_Evaluate( const in vec3 N, const in vec3 V, const in vec3 P, const in mat3 mInv, const in vec3 rectCoords[ 4 ] ) {
	vec3 v1 = rectCoords[ 1 ] - rectCoords[ 0 ];
	vec3 v2 = rectCoords[ 3 ] - rectCoords[ 0 ];
	vec3 lightNormal = cross( v1, v2 );
	if( dot( lightNormal, P - rectCoords[ 0 ] ) < 0.0 ) return vec3( 0.0 );
	vec3 T1, T2;
	T1 = normalize( V - N * dot( V, N ) );
	T2 = - cross( N, T1 );
	mat3 mat = mInv * transpose( mat3( T1, T2, N ) );
	vec3 coords[ 4 ];
	coords[ 0 ] = mat * ( rectCoords[ 0 ] - P );
	coords[ 1 ] = mat * ( rectCoords[ 1 ] - P );
	coords[ 2 ] = mat * ( rectCoords[ 2 ] - P );
	coords[ 3 ] = mat * ( rectCoords[ 3 ] - P );
	coords[ 0 ] = normalize( coords[ 0 ] );
	coords[ 1 ] = normalize( coords[ 1 ] );
	coords[ 2 ] = normalize( coords[ 2 ] );
	coords[ 3 ] = normalize( coords[ 3 ] );
	vec3 vectorFormFactor = vec3( 0.0 );
	vectorFormFactor += LTC_EdgeVectorFormFactor( coords[ 0 ], coords[ 1 ] );
	vectorFormFactor += LTC_EdgeVectorFormFactor( coords[ 1 ], coords[ 2 ] );
	vectorFormFactor += LTC_EdgeVectorFormFactor( coords[ 2 ], coords[ 3 ] );
	vectorFormFactor += LTC_EdgeVectorFormFactor( coords[ 3 ], coords[ 0 ] );
	float result = LTC_ClippedSphereFormFactor( vectorFormFactor );
	return vec3( result );
}
#if defined( USE_SHEEN )
float D_Charlie( float roughness, float dotNH ) {
	float alpha = pow2( roughness );
	float invAlpha = 1.0 / alpha;
	float cos2h = dotNH * dotNH;
	float sin2h = max( 1.0 - cos2h, 0.0078125 );
	return ( 2.0 + invAlpha ) * pow( sin2h, invAlpha * 0.5 ) / ( 2.0 * PI );
}
float V_Neubelt( float dotNV, float dotNL ) {
	return saturate( 1.0 / ( 4.0 * ( dotNL + dotNV - dotNL * dotNV ) ) );
}
vec3 BRDF_Sheen( const in vec3 lightDir, const in vec3 viewDir, const in vec3 normal, vec3 sheenColor, const in float sheenRoughness ) {
	vec3 halfDir = normalize( lightDir + viewDir );
	float dotNL = saturate( dot( normal, lightDir ) );
	float dotNV = saturate( dot( normal, viewDir ) );
	float dotNH = saturate( dot( normal, halfDir ) );
	float D = D_Charlie( sheenRoughness, dotNH );
	float V = V_Neubelt( dotNV, dotNL );
	return sheenColor * ( D * V );
}
#endif
float IBLSheenBRDF( const in vec3 normal, const in vec3 viewDir, const in float roughness ) {
	float dotNV = saturate( dot( normal, viewDir ) );
	float r2 = roughness * roughness;
	float rInv = 1.0 / ( roughness + 0.1 );
	float a = -1.9362 + 1.0678 * roughness + 0.4573 * r2 - 0.8469 * rInv;
	float b = -0.6014 + 0.5538 * roughness - 0.4670 * r2 - 0.1255 * rInv;
	float DG = exp( a * dotNV + b );
	return saturate( DG );
}
vec3 EnvironmentBRDF( const in vec3 normal, const in vec3 viewDir, const in vec3 specularColor, const in float specularF90, const in float roughness ) {
	float dotNV = saturate( dot( normal, viewDir ) );
	vec2 fab = texture2D( dfgLUT, vec2( roughness, dotNV ) ).rg;
	return specularColor * fab.x + specularF90 * fab.y;
}
#ifdef USE_IRIDESCENCE
void computeMultiscatteringIridescence( const in vec2 fab, const in vec3 specularColor, const in float specularF90, const in float iridescence, const in vec3 iridescenceF0, inout vec3 singleScatter, inout vec3 multiScatter ) {
#else
void computeMultiscattering( const in vec2 fab, const in vec3 specularColor, const in float specularF90, inout vec3 singleScatter, inout vec3 multiScatter ) {
#endif
	#ifdef USE_IRIDESCENCE
		vec3 Fr = mix( specularColor, iridescenceF0, iridescence );
	#else
		vec3 Fr = specularColor;
	#endif
	vec3 FssEss = Fr * fab.x + specularF90 * fab.y;
	float Ess = fab.x + fab.y;
	float Ems = 1.0 - Ess;
	vec3 Favg = Fr + ( 1.0 - Fr ) * 0.047619;	vec3 Fms = FssEss * Favg / ( 1.0 - Ems * Favg );
	singleScatter += FssEss;
	multiScatter += Fms * Ems;
}
#if NUM_RECT_AREA_LIGHTS > 0
	void RE_Direct_RectArea_Physical( const in RectAreaLight rectAreaLight, const in vec3 geometryPosition, const in vec3 geometryNormal, const in vec3 geometryViewDir, const in vec3 geometryClearcoatNormal, const in PhysicalMaterial material, inout ReflectedLight reflectedLight ) {
		vec3 normal = geometryNormal;
		vec3 viewDir = geometryViewDir;
		vec3 position = geometryPosition;
		vec3 lightPos = rectAreaLight.position;
		vec3 halfWidth = rectAreaLight.halfWidth;
		vec3 halfHeight = rectAreaLight.halfHeight;
		vec3 lightColor = rectAreaLight.color;
		float roughness = material.roughness;
		vec3 rectCoords[ 4 ];
		rectCoords[ 0 ] = lightPos + halfWidth - halfHeight;		rectCoords[ 1 ] = lightPos - halfWidth - halfHeight;
		rectCoords[ 2 ] = lightPos - halfWidth + halfHeight;
		rectCoords[ 3 ] = lightPos + halfWidth + halfHeight;
		vec2 uv = LTC_Uv( normal, viewDir, roughness );
		vec4 t1 = texture2D( ltc_1, uv );
		vec4 t2 = texture2D( ltc_2, uv );
		mat3 mInv = mat3(
			vec3( t1.x, 0, t1.y ),
			vec3(    0, 1,    0 ),
			vec3( t1.z, 0, t1.w )
		);
		vec3 fresnel = ( material.specularColorBlended * t2.x + ( material.specularF90 - material.specularColorBlended ) * t2.y );
		reflectedLight.directSpecular += lightColor * fresnel * LTC_Evaluate( normal, viewDir, position, mInv, rectCoords );
		reflectedLight.directDiffuse += lightColor * material.diffuseContribution * LTC_Evaluate( normal, viewDir, position, mat3( 1.0 ), rectCoords );
		#ifdef USE_CLEARCOAT
			vec3 Ncc = geometryClearcoatNormal;
			vec2 uvClearcoat = LTC_Uv( Ncc, viewDir, material.clearcoatRoughness );
			vec4 t1Clearcoat = texture2D( ltc_1, uvClearcoat );
			vec4 t2Clearcoat = texture2D( ltc_2, uvClearcoat );
			mat3 mInvClearcoat = mat3(
				vec3( t1Clearcoat.x, 0, t1Clearcoat.y ),
				vec3(             0, 1,             0 ),
				vec3( t1Clearcoat.z, 0, t1Clearcoat.w )
			);
			vec3 fresnelClearcoat = material.clearcoatF0 * t2Clearcoat.x + ( material.clearcoatF90 - material.clearcoatF0 ) * t2Clearcoat.y;
			clearcoatSpecularDirect += lightColor * fresnelClearcoat * LTC_Evaluate( Ncc, viewDir, position, mInvClearcoat, rectCoords );
		#endif
	}
#endif
void RE_Direct_Physical( const in IncidentLight directLight, const in vec3 geometryPosition, const in vec3 geometryNormal, const in vec3 geometryViewDir, const in vec3 geometryClearcoatNormal, const in PhysicalMaterial material, inout ReflectedLight reflectedLight ) {
	float dotNL = saturate( dot( geometryNormal, directLight.direction ) );
	vec3 irradiance = dotNL * directLight.color;
	#ifdef USE_CLEARCOAT
		float dotNLcc = saturate( dot( geometryClearcoatNormal, directLight.direction ) );
		vec3 ccIrradiance = dotNLcc * directLight.color;
		clearcoatSpecularDirect += ccIrradiance * BRDF_GGX_Clearcoat( directLight.direction, geometryViewDir, geometryClearcoatNormal, material );
	#endif
	#ifdef USE_SHEEN
 
 		sheenSpecularDirect += irradiance * BRDF_Sheen( directLight.direction, geometryViewDir, geometryNormal, material.sheenColor, material.sheenRoughness );
 
 		float sheenAlbedoV = IBLSheenBRDF( geometryNormal, geometryViewDir, material.sheenRoughness );
 		float sheenAlbedoL = IBLSheenBRDF( geometryNormal, directLight.direction, material.sheenRoughness );
 
 		float sheenEnergyComp = 1.0 - max3( material.sheenColor ) * max( sheenAlbedoV, sheenAlbedoL );
 
 		irradiance *= sheenEnergyComp;
 
 	#endif
	vec3 specularBRDF = BRDF_GGX( directLight.direction, geometryViewDir, geometryNormal, material );
	#ifdef USE_RETROREFLECTION
		vec3 retroViewDir = reflect( - geometryViewDir, geometryNormal );
		vec3 retroSpecularBRDF = BRDF_GGX( directLight.direction, retroViewDir, geometryNormal, material );
		specularBRDF = mix( specularBRDF, retroSpecularBRDF, saturate( material.retroreflectivity ) );
	#endif
	reflectedLight.directSpecular += irradiance * specularBRDF * material.multiScatteringCompensation;
	vec3 halfDir = normalize( directLight.direction + geometryViewDir );
	float dotVH = saturate( dot( geometryViewDir, halfDir ) );
	vec3 F = F_Schlick( material.specularColor, material.specularF90, dotVH );
	#ifdef USE_RETROREFLECTION
		vec3 retroHalfDir = normalize( directLight.direction + retroViewDir );
		float dotRetroVH = saturate( dot( retroViewDir, retroHalfDir ) );
		vec3 retroF = F_Schlick( material.specularColor, material.specularF90, dotRetroVH );
		F = mix( F, retroF, saturate( material.retroreflectivity ) );
	#endif
	reflectedLight.directDiffuse += irradiance * BRDF_Lambert( material.diffuseContribution ) * ( 1.0 - F );
}
void RE_IndirectDiffuse_Physical( const in vec3 irradiance, const in vec3 geometryPosition, const in vec3 geometryNormal, const in vec3 geometryViewDir, const in vec3 geometryClearcoatNormal, const in PhysicalMaterial material, inout ReflectedLight reflectedLight ) {
	vec3 singleScattering = vec3( 0.0 );
	vec3 multiScattering = vec3( 0.0 );
	#ifdef USE_IRIDESCENCE
		computeMultiscatteringIridescence( material.dfg, material.specularColor, material.specularF90, material.iridescence, material.iridescenceF0Dielectric, singleScattering, multiScattering );
	#else
		computeMultiscattering( material.dfg, material.specularColor, material.specularF90, singleScattering, multiScattering );
	#endif
	vec3 diffuse = irradiance * BRDF_Lambert( material.diffuseContribution ) * ( 1.0 - singleScattering - multiScattering );
	#ifdef USE_SHEEN
		float sheenAlbedo = IBLSheenBRDF( geometryNormal, geometryViewDir, material.sheenRoughness );
		sheenSpecularIndirect += irradiance * material.sheenColor * sheenAlbedo * RECIPROCAL_PI;
		float sheenEnergyComp = 1.0 - max3( material.sheenColor ) * sheenAlbedo;
		diffuse *= sheenEnergyComp;
	#endif
	reflectedLight.indirectDiffuse += diffuse;
}
void RE_IndirectSpecular_Physical( const in vec3 radiance, const in vec3 irradiance, const in vec3 clearcoatRadiance, const in vec3 geometryPosition, const in vec3 geometryNormal, const in vec3 geometryViewDir, const in vec3 geometryClearcoatNormal, const in PhysicalMaterial material, inout ReflectedLight reflectedLight) {
	#ifdef USE_CLEARCOAT
		clearcoatSpecularIndirect += clearcoatRadiance * EnvironmentBRDF( geometryClearcoatNormal, geometryViewDir, material.clearcoatF0, material.clearcoatF90, material.clearcoatRoughness );
	#endif
	#ifdef USE_SHEEN
		sheenSpecularIndirect += irradiance * material.sheenColor * IBLSheenBRDF( geometryNormal, geometryViewDir, material.sheenRoughness ) * RECIPROCAL_PI;
 	#endif
	vec3 singleScatteringDielectric = vec3( 0.0 );
	vec3 multiScatteringDielectric = vec3( 0.0 );
	vec3 singleScatteringMetallic = vec3( 0.0 );
	vec3 multiScatteringMetallic = vec3( 0.0 );
	#ifdef USE_IRIDESCENCE
		computeMultiscatteringIridescence( material.dfg, material.specularColor, material.specularF90, material.iridescence, material.iridescenceF0Dielectric, singleScatteringDielectric, multiScatteringDielectric );
		computeMultiscatteringIridescence( material.dfg, material.diffuseColor, material.specularF90, material.iridescence, material.iridescenceF0Metallic, singleScatteringMetallic, multiScatteringMetallic );
	#else
		computeMultiscattering( material.dfg, material.specularColor, material.specularF90, singleScatteringDielectric, multiScatteringDielectric );
		computeMultiscattering( material.dfg, material.diffuseColor, material.specularF90, singleScatteringMetallic, multiScatteringMetallic );
	#endif
	vec3 singleScattering = mix( singleScatteringDielectric, singleScatteringMetallic, material.metalness );
	vec3 multiScattering = mix( multiScatteringDielectric, multiScatteringMetallic, material.metalness );
	vec3 totalScatteringDielectric = singleScatteringDielectric + multiScatteringDielectric;
	vec3 diffuse = material.diffuseContribution * ( 1.0 - totalScatteringDielectric );
	vec3 cosineWeightedIrradiance = irradiance * RECIPROCAL_PI;
	vec3 indirectSpecular = radiance * singleScattering;
	indirectSpecular += multiScattering * cosineWeightedIrradiance;
	vec3 indirectDiffuse = diffuse * cosineWeightedIrradiance;
	#ifdef USE_SHEEN
		float sheenAlbedo = IBLSheenBRDF( geometryNormal, geometryViewDir, material.sheenRoughness );
		float sheenEnergyComp = 1.0 - max3( material.sheenColor ) * sheenAlbedo;
		indirectSpecular *= sheenEnergyComp;
		indirectDiffuse *= sheenEnergyComp;
	#endif
	reflectedLight.indirectSpecular += indirectSpecular;
	reflectedLight.indirectDiffuse += indirectDiffuse;
}
#define RE_Direct				RE_Direct_Physical
#define RE_Direct_RectArea		RE_Direct_RectArea_Physical
#define RE_IndirectDiffuse		RE_IndirectDiffuse_Physical
#define RE_IndirectSpecular		RE_IndirectSpecular_Physical
float computeSpecularOcclusion( const in float dotNV, const in float ambientOcclusion, const in float roughness ) {
	return saturate( pow( dotNV + ambientOcclusion, exp2( - 16.0 * roughness - 1.0 ) ) - 1.0 + ambientOcclusion );
}`,gb=`
vec3 geometryPosition = - vViewPosition;
vec3 geometryNormal = normal;
vec3 geometryViewDir = ( isOrthographic ) ? vec3( 0, 0, 1 ) : normalize( vViewPosition );
vec3 geometryClearcoatNormal = vec3( 0.0 );
#ifdef USE_CLEARCOAT
	geometryClearcoatNormal = clearcoatNormal;
#endif
#ifdef USE_IRIDESCENCE
	float dotNVi = saturate( dot( normal, geometryViewDir ) );
	if ( material.iridescenceThickness == 0.0 ) {
		material.iridescence = 0.0;
	} else {
		material.iridescence = saturate( material.iridescence );
	}
	if ( material.iridescence > 0.0 ) {
		vec3 iridescenceFresnelDielectric = evalIridescence( 1.0, material.iridescenceIOR, dotNVi, material.iridescenceThickness, material.specularColor );
		vec3 iridescenceFresnelMetallic = evalIridescence( 1.0, material.iridescenceIOR, dotNVi, material.iridescenceThickness, material.diffuseColor );
		material.iridescenceFresnel = mix( iridescenceFresnelDielectric, iridescenceFresnelMetallic, material.metalness );
		material.iridescenceF0Dielectric = Schlick_to_F0( iridescenceFresnelDielectric, 1.0, dotNVi );
		material.iridescenceF0Metallic = Schlick_to_F0( iridescenceFresnelMetallic, 1.0, dotNVi );
	}
#endif
#ifdef STANDARD
	float dotNVms = saturate( dot( geometryNormal, geometryViewDir ) );
	material.dfg = texture2D( dfgLUT, vec2( material.roughness, dotNVms ) ).rg;
	#if ( NUM_SUN_LIGHTS > 0 || NUM_DIR_LIGHTS > 0 || NUM_POINT_LIGHTS > 0 || NUM_SPOT_LIGHTS > 0 )
		float EssMs = material.dfg.x + material.dfg.y;
		material.multiScatteringCompensation = 1.0 + material.specularColorBlended * ( 1.0 / EssMs - 1.0 );
	#endif
#endif
IncidentLight directLight;
#if ( NUM_POINT_LIGHTS > 0 ) && defined( RE_Direct )
	PointLight pointLight;
	#if defined( USE_SHADOWMAP ) && NUM_POINT_LIGHT_SHADOWS > 0
	PointLightShadow pointLightShadow;
	#endif
	#pragma unroll_loop_start
	for ( int i = 0; i < NUM_POINT_LIGHTS; i ++ ) {
		pointLight = pointLights[ i ];
		getPointLightInfo( pointLight, geometryPosition, directLight );
		#if defined( USE_SHADOWMAP ) && ( UNROLLED_LOOP_INDEX < NUM_POINT_LIGHT_SHADOWS ) && ( defined( SHADOWMAP_TYPE_PCF ) || defined( SHADOWMAP_TYPE_BASIC ) )
		pointLightShadow = pointLightShadows[ i ];
		directLight.color *= ( directLight.visible && receiveShadow ) ? getPointShadow( pointShadowMap[ i ], pointLightShadow.shadowMapSize, pointLightShadow.shadowIntensity, pointLightShadow.shadowBias, pointLightShadow.shadowRadius, vPointShadowCoord[ i ], pointLightShadow.shadowCameraNear, pointLightShadow.shadowCameraFar ) : 1.0;
		#endif
		RE_Direct( directLight, geometryPosition, geometryNormal, geometryViewDir, geometryClearcoatNormal, material, reflectedLight );
	}
	#pragma unroll_loop_end
#endif
#if ( NUM_SPOT_LIGHTS > 0 ) && defined( RE_Direct )
	SpotLight spotLight;
	vec4 spotColor;
	vec3 spotLightCoord;
	bool inSpotLightMap;
	#if defined( USE_SHADOWMAP ) && NUM_SPOT_LIGHT_SHADOWS > 0
	SpotLightShadow spotLightShadow;
	#endif
	#pragma unroll_loop_start
	for ( int i = 0; i < NUM_SPOT_LIGHTS; i ++ ) {
		spotLight = spotLights[ i ];
		getSpotLightInfo( spotLight, geometryPosition, directLight );
		#if ( UNROLLED_LOOP_INDEX < NUM_SPOT_LIGHT_SHADOWS_WITH_MAPS )
		#define SPOT_LIGHT_MAP_INDEX UNROLLED_LOOP_INDEX
		#elif ( UNROLLED_LOOP_INDEX < NUM_SPOT_LIGHT_SHADOWS )
		#define SPOT_LIGHT_MAP_INDEX NUM_SPOT_LIGHT_MAPS
		#else
		#define SPOT_LIGHT_MAP_INDEX ( UNROLLED_LOOP_INDEX - NUM_SPOT_LIGHT_SHADOWS + NUM_SPOT_LIGHT_SHADOWS_WITH_MAPS )
		#endif
		#if ( SPOT_LIGHT_MAP_INDEX < NUM_SPOT_LIGHT_MAPS )
			spotLightCoord = vSpotLightCoord[ i ].xyz / vSpotLightCoord[ i ].w;
			inSpotLightMap = all( lessThan( abs( spotLightCoord * 2. - 1. ), vec3( 1.0 ) ) );
			spotColor = texture2D( spotLightMap[ SPOT_LIGHT_MAP_INDEX ], spotLightCoord.xy );
			directLight.color = inSpotLightMap ? directLight.color * spotColor.rgb : directLight.color;
		#endif
		#undef SPOT_LIGHT_MAP_INDEX
		#if defined( USE_SHADOWMAP ) && ( UNROLLED_LOOP_INDEX < NUM_SPOT_LIGHT_SHADOWS )
		spotLightShadow = spotLightShadows[ i ];
		directLight.color *= ( directLight.visible && receiveShadow ) ? getShadow( spotShadowMap[ i ], spotLightShadow.shadowMapSize, spotLightShadow.shadowIntensity, spotLightShadow.shadowBias, spotLightShadow.shadowRadius, vSpotLightCoord[ i ] ) : 1.0;
		#endif
		RE_Direct( directLight, geometryPosition, geometryNormal, geometryViewDir, geometryClearcoatNormal, material, reflectedLight );
	}
	#pragma unroll_loop_end
#endif
#if ( NUM_SUN_LIGHTS > 0 ) && defined( RE_Direct )
	SunLight sunLight;
	#if defined( USE_SHADOWMAP ) && NUM_SUN_LIGHT_SHADOWS > 0
	SunLightShadow sunLightShadow;
	#endif
	#pragma unroll_loop_start
	for ( int i = 0; i < NUM_SUN_LIGHTS; i ++ ) {
		sunLight = sunLights[ i ];
		getSunLightInfo( sunLight, directLight );
		#if defined( USE_SHADOWMAP ) && ( UNROLLED_LOOP_INDEX < NUM_SUN_LIGHT_SHADOWS )
		sunLightShadow = sunLightShadows[ i ];
		directLight.color *= ( directLight.visible && receiveShadow ) ? getSunShadow( sunShadowMap[ i ], sunLightShadow, UNROLLED_LOOP_INDEX ) : 1.0;
		#endif
		RE_Direct( directLight, geometryPosition, geometryNormal, geometryViewDir, geometryClearcoatNormal, material, reflectedLight );
	}
	#pragma unroll_loop_end
#endif
#if ( NUM_DIR_LIGHTS > 0 ) && defined( RE_Direct )
	DirectionalLight directionalLight;
	#if defined( USE_SHADOWMAP ) && NUM_DIR_LIGHT_SHADOWS > 0
	DirectionalLightShadow directionalLightShadow;
	#endif
	#pragma unroll_loop_start
	for ( int i = 0; i < NUM_DIR_LIGHTS; i ++ ) {
		directionalLight = directionalLights[ i ];
		getDirectionalLightInfo( directionalLight, directLight );
		#if defined( USE_SHADOWMAP ) && ( UNROLLED_LOOP_INDEX < NUM_DIR_LIGHT_SHADOWS )
		directionalLightShadow = directionalLightShadows[ i ];
		directLight.color *= ( directLight.visible && receiveShadow ) ? getShadow( directionalShadowMap[ i ], directionalLightShadow.shadowMapSize, directionalLightShadow.shadowIntensity, directionalLightShadow.shadowBias, directionalLightShadow.shadowRadius, vDirectionalShadowCoord[ i ] ) : 1.0;
		#endif
		RE_Direct( directLight, geometryPosition, geometryNormal, geometryViewDir, geometryClearcoatNormal, material, reflectedLight );
	}
	#pragma unroll_loop_end
#endif
#if ( NUM_RECT_AREA_LIGHTS > 0 ) && defined( RE_Direct_RectArea )
	RectAreaLight rectAreaLight;
	#pragma unroll_loop_start
	for ( int i = 0; i < NUM_RECT_AREA_LIGHTS; i ++ ) {
		rectAreaLight = rectAreaLights[ i ];
		RE_Direct_RectArea( rectAreaLight, geometryPosition, geometryNormal, geometryViewDir, geometryClearcoatNormal, material, reflectedLight );
	}
	#pragma unroll_loop_end
#endif
#if defined( RE_IndirectDiffuse )
	vec3 iblIrradiance = vec3( 0.0 );
	vec3 irradiance = getAmbientLightIrradiance( ambientLightColor );
	#if defined( USE_LIGHT_PROBES )
		irradiance += getLightProbeIrradiance( lightProbe, geometryNormal );
	#endif
	#if ( NUM_HEMI_LIGHTS > 0 )
		#pragma unroll_loop_start
		for ( int i = 0; i < NUM_HEMI_LIGHTS; i ++ ) {
			irradiance += getHemisphereLightIrradiance( hemisphereLights[ i ], geometryNormal );
		}
		#pragma unroll_loop_end
	#endif
	#ifdef USE_LIGHT_PROBES_GRID
		vec3 probeWorldPos = ( ( vec4( geometryPosition, 1.0 ) - viewMatrix[ 3 ] ) * viewMatrix ).xyz;
		vec3 probeWorldNormal = transformNormalByInverseViewMatrix( geometryNormal, viewMatrix );
		irradiance += getLightProbeGridIrradiance( probeWorldPos, probeWorldNormal );
	#endif
#endif
#if defined( RE_IndirectSpecular )
	vec3 radiance = vec3( 0.0 );
	vec3 clearcoatRadiance = vec3( 0.0 );
#endif`,xb=`#if defined( RE_IndirectDiffuse )
	#ifdef USE_LIGHTMAP
		vec4 lightMapTexel = texture2D( lightMap, vLightMapUv );
		vec3 lightMapIrradiance = lightMapTexel.rgb * lightMapIntensity;
		irradiance += lightMapIrradiance;
	#endif
	#if defined( USE_ENVMAP ) && defined( ENVMAP_TYPE_CUBE_UV )
		#if defined( STANDARD ) || defined( LAMBERT ) || defined( PHONG )
			iblIrradiance += getIBLIrradiance( geometryNormal );
		#endif
	#endif
#endif
#if defined( USE_ENVMAP ) && defined( RE_IndirectSpecular )
	#ifdef USE_ANISOTROPY
		vec3 iblRadiance = getIBLAnisotropyRadiance( geometryViewDir, geometryNormal, material.roughness, material.anisotropyB, material.anisotropy );
	#else
		vec3 iblRadiance = getIBLRadiance( geometryViewDir, geometryNormal, material.roughness );
	#endif
	#ifdef USE_RETROREFLECTION
		#ifdef USE_ANISOTROPY
			vec3 retroIBLRadiance = getIBLAnisotropyRetroRadiance( geometryViewDir, geometryNormal, material.roughness, material.anisotropyB, material.anisotropy );
		#else
			vec3 retroIBLRadiance = getIBLRetroRadiance( geometryViewDir, geometryNormal, material.roughness );
		#endif
		iblRadiance = mix( iblRadiance, retroIBLRadiance, saturate( material.retroreflectivity ) );
	#endif
	radiance += iblRadiance;
	#ifdef USE_CLEARCOAT
		clearcoatRadiance += getIBLRadiance( geometryViewDir, geometryClearcoatNormal, material.clearcoatRoughness );
	#endif
#endif`,yb=`#if defined( RE_IndirectDiffuse )
	#if defined( LAMBERT ) || defined( PHONG )
		irradiance += iblIrradiance;
	#endif
	RE_IndirectDiffuse( irradiance, geometryPosition, geometryNormal, geometryViewDir, geometryClearcoatNormal, material, reflectedLight );
#endif
#if defined( RE_IndirectSpecular )
	RE_IndirectSpecular( radiance, iblIrradiance, clearcoatRadiance, geometryPosition, geometryNormal, geometryViewDir, geometryClearcoatNormal, material, reflectedLight );
#endif`,_b=`#ifdef USE_LIGHT_PROBES_GRID
uniform highp sampler3D probesSH;
uniform vec3 probesMin;
uniform vec3 probesMax;
uniform vec3 probesResolution;
vec3 getLightProbeGridIrradiance( vec3 worldPos, vec3 worldNormal ) {
	vec3 res = probesResolution;
	vec3 gridRange = probesMax - probesMin;
	vec3 resMinusOne = res - 1.0;
	vec3 probeSpacing = gridRange / resMinusOne;
	vec3 samplePos = worldPos + worldNormal * probeSpacing * 0.5;
	vec3 uvw = clamp( ( samplePos - probesMin ) / gridRange, 0.0, 1.0 );
	uvw = uvw * resMinusOne / res + 0.5 / res;
	float nz          = res.z;
	float paddedSlices = nz + 2.0;
	float atlasDepth  = 7.0 * paddedSlices;
	float uvZBase     = uvw.z * nz + 1.0;
	vec4 s0 = texture( probesSH, vec3( uvw.xy, ( uvZBase                       ) / atlasDepth ) );
	vec4 s1 = texture( probesSH, vec3( uvw.xy, ( uvZBase +       paddedSlices   ) / atlasDepth ) );
	vec4 s2 = texture( probesSH, vec3( uvw.xy, ( uvZBase + 2.0 * paddedSlices   ) / atlasDepth ) );
	vec4 s3 = texture( probesSH, vec3( uvw.xy, ( uvZBase + 3.0 * paddedSlices   ) / atlasDepth ) );
	vec4 s4 = texture( probesSH, vec3( uvw.xy, ( uvZBase + 4.0 * paddedSlices   ) / atlasDepth ) );
	vec4 s5 = texture( probesSH, vec3( uvw.xy, ( uvZBase + 5.0 * paddedSlices   ) / atlasDepth ) );
	vec4 s6 = texture( probesSH, vec3( uvw.xy, ( uvZBase + 6.0 * paddedSlices   ) / atlasDepth ) );
	vec3 c0 = s0.xyz;
	vec3 c1 = vec3( s0.w, s1.xy );
	vec3 c2 = vec3( s1.zw, s2.x );
	vec3 c3 = s2.yzw;
	vec3 c4 = s3.xyz;
	vec3 c5 = vec3( s3.w, s4.xy );
	vec3 c6 = vec3( s4.zw, s5.x );
	vec3 c7 = s5.yzw;
	vec3 c8 = s6.xyz;
	float x = worldNormal.x, y = worldNormal.y, z = worldNormal.z;
	vec3 result = c0 * 0.886227;
	result += c1 * 2.0 * 0.511664 * y;
	result += c2 * 2.0 * 0.511664 * z;
	result += c3 * 2.0 * 0.511664 * x;
	result += c4 * 2.0 * 0.429043 * x * y;
	result += c5 * 2.0 * 0.429043 * y * z;
	result += c6 * ( 0.743125 * z * z - 0.247708 );
	result += c7 * 2.0 * 0.429043 * x * z;
	result += c8 * 0.429043 * ( x * x - y * y );
	return max( result, vec3( 0.0 ) );
}
#endif`,bb=`#if defined( USE_LOGARITHMIC_DEPTH_BUFFER )
	gl_FragDepth = vIsPerspective == 0.0 ? gl_FragCoord.z : log2( vFragDepth ) * logDepthBufFC * 0.5;
#endif`,Sb=`#if defined( USE_LOGARITHMIC_DEPTH_BUFFER )
	uniform float logDepthBufFC;
	varying float vFragDepth;
	varying float vIsPerspective;
#endif`,vb=`#ifdef USE_LOGARITHMIC_DEPTH_BUFFER
	varying float vFragDepth;
	varying float vIsPerspective;
#endif`,Ab=`#ifdef USE_LOGARITHMIC_DEPTH_BUFFER
	vFragDepth = 1.0 + gl_Position.w;
	vIsPerspective = float( isPerspectiveMatrix( projectionMatrix ) );
#endif`,Tb=`#ifdef USE_MAP
	vec4 sampledDiffuseColor = texture2D( map, vMapUv );
	#ifdef DECODE_VIDEO_TEXTURE
		sampledDiffuseColor = sRGBTransferEOTF( sampledDiffuseColor );
	#endif
	diffuseColor *= sampledDiffuseColor;
#endif`,Eb=`#ifdef USE_MAP
	uniform sampler2D map;
#endif`,Mb=`#if defined( USE_MAP ) || defined( USE_ALPHAMAP )
	#if defined( USE_POINTS_UV )
		vec2 uv = vUv;
	#else
		vec2 uv = ( uvTransform * vec3( gl_PointCoord.x, 1.0 - gl_PointCoord.y, 1 ) ).xy;
	#endif
#endif
#ifdef USE_MAP
	diffuseColor *= texture2D( map, uv );
#endif
#ifdef USE_ALPHAMAP
	diffuseColor.a *= texture2D( alphaMap, uv ).g;
#endif`,Rb=`#if defined( USE_POINTS_UV )
	varying vec2 vUv;
#else
	#if defined( USE_MAP ) || defined( USE_ALPHAMAP )
		uniform mat3 uvTransform;
	#endif
#endif
#ifdef USE_MAP
	uniform sampler2D map;
#endif
#ifdef USE_ALPHAMAP
	uniform sampler2D alphaMap;
#endif`,wb=`float metalnessFactor = metalness;
#ifdef USE_METALNESSMAP
	vec4 texelMetalness = texture2D( metalnessMap, vMetalnessMapUv );
	metalnessFactor *= texelMetalness.b;
#endif`,Cb=`#ifdef USE_METALNESSMAP
	uniform sampler2D metalnessMap;
#endif`,Nb=`#ifdef USE_INSTANCING_MORPH
	float morphTargetInfluences[ MORPHTARGETS_COUNT ];
	float morphTargetBaseInfluence = texelFetch( morphTexture, ivec2( 0, gl_InstanceID ), 0 ).r;
	for ( int i = 0; i < MORPHTARGETS_COUNT; i ++ ) {
		morphTargetInfluences[i] =  texelFetch( morphTexture, ivec2( i + 1, gl_InstanceID ), 0 ).r;
	}
#endif`,Ib=`#if defined( USE_MORPHCOLORS )
	vColor *= morphTargetBaseInfluence;
	for ( int i = 0; i < MORPHTARGETS_COUNT; i ++ ) {
		#if defined( USE_COLOR_ALPHA )
			if ( morphTargetInfluences[ i ] != 0.0 ) vColor += getMorph( gl_VertexID, i, 2 ) * morphTargetInfluences[ i ];
		#elif defined( USE_COLOR )
			if ( morphTargetInfluences[ i ] != 0.0 ) vColor += getMorph( gl_VertexID, i, 2 ).rgb * morphTargetInfluences[ i ];
		#endif
	}
#endif`,Bb=`#ifdef USE_MORPHNORMALS
	objectNormal *= morphTargetBaseInfluence;
	for ( int i = 0; i < MORPHTARGETS_COUNT; i ++ ) {
		if ( morphTargetInfluences[ i ] != 0.0 ) objectNormal += getMorph( gl_VertexID, i, 1 ).xyz * morphTargetInfluences[ i ];
	}
#endif`,Fb=`#ifdef USE_MORPHTARGETS
	#ifndef USE_INSTANCING_MORPH
		uniform float morphTargetBaseInfluence;
		uniform float morphTargetInfluences[ MORPHTARGETS_COUNT ];
	#endif
	uniform sampler2DArray morphTargetsTexture;
	uniform ivec2 morphTargetsTextureSize;
	vec4 getMorph( const in int vertexIndex, const in int morphTargetIndex, const in int offset ) {
		int texelIndex = vertexIndex * MORPHTARGETS_TEXTURE_STRIDE + offset;
		int y = texelIndex / morphTargetsTextureSize.x;
		int x = texelIndex - y * morphTargetsTextureSize.x;
		ivec3 morphUV = ivec3( x, y, morphTargetIndex );
		return texelFetch( morphTargetsTexture, morphUV, 0 );
	}
#endif`,Lb=`#ifdef USE_MORPHTARGETS
	transformed *= morphTargetBaseInfluence;
	for ( int i = 0; i < MORPHTARGETS_COUNT; i ++ ) {
		if ( morphTargetInfluences[ i ] != 0.0 ) transformed += getMorph( gl_VertexID, i, 0 ).xyz * morphTargetInfluences[ i ];
	}
#endif`,Ub=`float faceDirection = gl_FrontFacing ? 1.0 : - 1.0;
#ifdef FLAT_SHADED
	vec3 fdx = dFdx( vViewPosition );
	vec3 fdy = dFdy( vViewPosition );
	vec3 normal = normalize( cross( fdx, fdy ) );
#else
	vec3 normal = normalize( vNormal );
	#ifdef DOUBLE_SIDED
		normal *= faceDirection;
	#endif
#endif
#if defined( USE_NORMALMAP_TANGENTSPACE ) || defined( USE_CLEARCOAT_NORMALMAP ) || defined( USE_ANISOTROPY )
	#ifdef USE_TANGENT
		mat3 tbn = mat3( normalize( vTangent ), normalize( vBitangent ), normal );
	#else
		mat3 tbn = getTangentFrame( - vViewPosition, normal,
		#if defined( USE_NORMALMAP )
			vNormalMapUv
		#elif defined( USE_CLEARCOAT_NORMALMAP )
			vClearcoatNormalMapUv
		#else
			vUv
		#endif
		);
	#endif
	#ifdef DOUBLE_SIDED
		tbn[0] *= faceDirection;
		tbn[1] *= faceDirection;
	#endif
#endif
#ifdef USE_CLEARCOAT_NORMALMAP
	#ifdef USE_TANGENT
		mat3 tbn2 = mat3( normalize( vTangent ), normalize( vBitangent ), normal );
	#else
		mat3 tbn2 = getTangentFrame( - vViewPosition, normal, vClearcoatNormalMapUv );
	#endif
	#ifdef DOUBLE_SIDED
		tbn2[0] *= faceDirection;
		tbn2[1] *= faceDirection;
	#endif
#endif
vec3 nonPerturbedNormal = normal;`,Db=`#ifdef USE_NORMALMAP_OBJECTSPACE
	normal = texture2D( normalMap, vNormalMapUv ).xyz * 2.0 - 1.0;
	#ifdef FLIP_SIDED
		normal = - normal;
	#endif
	#ifdef DOUBLE_SIDED
		normal = normal * faceDirection;
	#endif
	normal = normalize( normalMatrix * normal );
#elif defined( USE_NORMALMAP_TANGENTSPACE )
	vec3 mapN = texture2D( normalMap, vNormalMapUv ).xyz * 2.0 - 1.0;
	#if defined( USE_PACKED_NORMALMAP )
		mapN = vec3( mapN.xy, sqrt( saturate( 1.0 - dot( mapN.xy, mapN.xy ) ) ) );
	#endif
	mapN.xy *= normalScale;
	normal = normalize( tbn * mapN );
#elif defined( USE_BUMPMAP )
	normal = perturbNormalArb( - vViewPosition, normal, dHdxy_fwd(), faceDirection );
#endif`,Pb=`#ifndef FLAT_SHADED
	varying vec3 vNormal;
	#ifdef USE_TANGENT
		varying vec3 vTangent;
		varying vec3 vBitangent;
	#endif
#endif`,Gb=`#ifndef FLAT_SHADED
	varying vec3 vNormal;
	#ifdef USE_TANGENT
		varying vec3 vTangent;
		varying vec3 vBitangent;
	#endif
#endif`,Vb=`#ifndef FLAT_SHADED
	vNormal = normalize( transformedNormal );
	#ifdef USE_TANGENT
		vTangent = normalize( transformedTangent );
		vBitangent = normalize( cross( vNormal, vTangent ) * tangent.w );
		#ifdef FLIP_SIDED
			vBitangent = - vBitangent;
		#endif
	#endif
#endif`,Ob=`#ifdef USE_NORMALMAP
	uniform sampler2D normalMap;
	uniform vec2 normalScale;
#endif
#ifdef USE_NORMALMAP_OBJECTSPACE
	uniform mat3 normalMatrix;
#endif
#if ! defined ( USE_TANGENT ) && ( defined ( USE_NORMALMAP_TANGENTSPACE ) || defined ( USE_CLEARCOAT_NORMALMAP ) || defined( USE_ANISOTROPY ) )
	mat3 getTangentFrame( vec3 eye_pos, vec3 surf_norm, vec2 uv ) {
		vec3 q0 = dFdx( eye_pos.xyz );
		vec3 q1 = dFdy( eye_pos.xyz );
		vec2 st0 = dFdx( uv.st );
		vec2 st1 = dFdy( uv.st );
		vec3 N = surf_norm;
		vec3 q1perp = cross( q1, N );
		vec3 q0perp = cross( N, q0 );
		vec3 T = q1perp * st0.x + q0perp * st1.x;
		vec3 B = q1perp * st0.y + q0perp * st1.y;
		float det = max( dot( T, T ), dot( B, B ) );
		float scale = ( det == 0.0 ) ? 0.0 : inversesqrt( det );
		return mat3( T * scale, B * scale, N );
	}
#endif`,kb=`#ifdef USE_CLEARCOAT
	vec3 clearcoatNormal = nonPerturbedNormal;
#endif`,zb=`#ifdef USE_CLEARCOAT_NORMALMAP
	vec3 clearcoatMapN = texture2D( clearcoatNormalMap, vClearcoatNormalMapUv ).xyz * 2.0 - 1.0;
	clearcoatMapN.xy *= clearcoatNormalScale;
	clearcoatNormal = normalize( tbn2 * clearcoatMapN );
#endif`,Wb=`#ifdef USE_CLEARCOATMAP
	uniform sampler2D clearcoatMap;
#endif
#ifdef USE_CLEARCOAT_NORMALMAP
	uniform sampler2D clearcoatNormalMap;
	uniform vec2 clearcoatNormalScale;
#endif
#ifdef USE_CLEARCOAT_ROUGHNESSMAP
	uniform sampler2D clearcoatRoughnessMap;
#endif`,Hb=`#ifdef USE_IRIDESCENCEMAP
	uniform sampler2D iridescenceMap;
#endif
#ifdef USE_IRIDESCENCE_THICKNESSMAP
	uniform sampler2D iridescenceThicknessMap;
#endif`,Yb=`#ifdef OPAQUE
diffuseColor.a = 1.0;
#endif
#ifdef USE_TRANSMISSION
diffuseColor.a *= material.transmissionAlpha;
#endif
gl_FragColor = vec4( outgoingLight, diffuseColor.a );`,Zb=`vec3 packNormalToRGB( const in vec3 normal ) {
	return normalize( normal ) * 0.5 + 0.5;
}
vec3 unpackRGBToNormal( const in vec3 rgb ) {
	return 2.0 * rgb.xyz - 1.0;
}
const float PackUpscale = 256. / 255.;const float UnpackDownscale = 255. / 256.;const float ShiftRight8 = 1. / 256.;
const float Inv255 = 1. / 255.;
const vec4 PackFactors = vec4( 1.0, 256.0, 256.0 * 256.0, 256.0 * 256.0 * 256.0 );
const vec2 UnpackFactors2 = vec2( UnpackDownscale, 1.0 / PackFactors.g );
const vec3 UnpackFactors3 = vec3( UnpackDownscale / PackFactors.rg, 1.0 / PackFactors.b );
const vec4 UnpackFactors4 = vec4( UnpackDownscale / PackFactors.rgb, 1.0 / PackFactors.a );
vec4 packDepthToRGBA( const in float v ) {
	if( v <= 0.0 )
		return vec4( 0., 0., 0., 0. );
	if( v >= 1.0 )
		return vec4( 1., 1., 1., 1. );
	float vuf;
	float af = modf( v * PackFactors.a, vuf );
	float bf = modf( vuf * ShiftRight8, vuf );
	float gf = modf( vuf * ShiftRight8, vuf );
	return vec4( vuf * Inv255, gf * PackUpscale, bf * PackUpscale, af );
}
vec3 packDepthToRGB( const in float v ) {
	if( v <= 0.0 )
		return vec3( 0., 0., 0. );
	if( v >= 1.0 )
		return vec3( 1., 1., 1. );
	float vuf;
	float bf = modf( v * PackFactors.b, vuf );
	float gf = modf( vuf * ShiftRight8, vuf );
	return vec3( vuf * Inv255, gf * PackUpscale, bf );
}
vec2 packDepthToRG( const in float v ) {
	if( v <= 0.0 )
		return vec2( 0., 0. );
	if( v >= 1.0 )
		return vec2( 1., 1. );
	float vuf;
	float gf = modf( v * 256., vuf );
	return vec2( vuf * Inv255, gf );
}
float unpackRGBAToDepth( const in vec4 v ) {
	return dot( v, UnpackFactors4 );
}
float unpackRGBToDepth( const in vec3 v ) {
	return dot( v, UnpackFactors3 );
}
float unpackRGToDepth( const in vec2 v ) {
	return v.r * UnpackFactors2.r + v.g * UnpackFactors2.g;
}
vec4 pack2HalfToRGBA( const in vec2 v ) {
	vec4 r = vec4( v.x, fract( v.x * 255.0 ), v.y, fract( v.y * 255.0 ) );
	return vec4( r.x - r.y / 255.0, r.y, r.z - r.w / 255.0, r.w );
}
vec2 unpackRGBATo2Half( const in vec4 v ) {
	return vec2( v.x + ( v.y / 255.0 ), v.z + ( v.w / 255.0 ) );
}
float viewZToOrthographicDepth( const in float viewZ, const in float near, const in float far ) {
	return ( viewZ + near ) / ( near - far );
}
float orthographicDepthToViewZ( const in float depth, const in float near, const in float far ) {
	#ifdef USE_REVERSED_DEPTH_BUFFER
	
		return depth * ( far - near ) - far;
	#else
		return depth * ( near - far ) - near;
	#endif
}
float viewZToPerspectiveDepth( const in float viewZ, const in float near, const in float far ) {
	return ( ( near + viewZ ) * far ) / ( ( far - near ) * viewZ );
}
float perspectiveDepthToViewZ( const in float depth, const in float near, const in float far ) {
	
	#ifdef USE_REVERSED_DEPTH_BUFFER
		return ( near * far ) / ( ( near - far ) * depth - near );
	#else
		return ( near * far ) / ( ( far - near ) * depth - far );
	#endif
}`,Xb=`#ifdef PREMULTIPLIED_ALPHA
	gl_FragColor.rgb *= gl_FragColor.a;
#endif`,Kb=`vec4 mvPosition = vec4( transformed, 1.0 );
#ifdef USE_BATCHING
	mvPosition = batchingMatrix * mvPosition;
#endif
#ifdef USE_INSTANCING
	mvPosition = instanceMatrix * mvPosition;
#endif
mvPosition = modelViewMatrix * mvPosition;
gl_Position = projectionMatrix * mvPosition;`,$b=`#ifdef DITHERING
	gl_FragColor.rgb = dithering( gl_FragColor.rgb );
#endif`,Jb=`#ifdef DITHERING
	vec3 dithering( vec3 color ) {
		float grid_position = rand( gl_FragCoord.xy );
		vec3 dither_shift_RGB = vec3( 0.25 / 255.0, -0.25 / 255.0, 0.25 / 255.0 );
		dither_shift_RGB = mix( 2.0 * dither_shift_RGB, -2.0 * dither_shift_RGB, grid_position );
		return color + dither_shift_RGB;
	}
#endif`,qb=`float roughnessFactor = roughness;
#ifdef USE_ROUGHNESSMAP
	vec4 texelRoughness = texture2D( roughnessMap, vRoughnessMapUv );
	roughnessFactor *= texelRoughness.g;
#endif`,Qb=`#ifdef USE_ROUGHNESSMAP
	uniform sampler2D roughnessMap;
#endif`,jb=`#if NUM_SPOT_LIGHT_COORDS > 0
	varying vec4 vSpotLightCoord[ NUM_SPOT_LIGHT_COORDS ];
#endif
#if NUM_SPOT_LIGHT_MAPS > 0
	uniform sampler2D spotLightMap[ NUM_SPOT_LIGHT_MAPS ];
#endif
#ifdef USE_SHADOWMAP
	#if NUM_SUN_LIGHT_SHADOWS > 0
		#define SUN_LIGHT_CASCADES 2
		#if defined( SHADOWMAP_TYPE_PCF )
			uniform sampler2DShadow sunShadowMap[ NUM_SUN_LIGHT_SHADOWS ];
		#else
			uniform sampler2D sunShadowMap[ NUM_SUN_LIGHT_SHADOWS ];
		#endif
		uniform mat4 sunShadowMatrix[ NUM_SUN_LIGHT_SHADOWS * SUN_LIGHT_CASCADES ];
		uniform vec4 sunShadowCascade[ NUM_SUN_LIGHT_SHADOWS * SUN_LIGHT_CASCADES ];
		varying vec4 vSunShadowWorldPosition;
		varying vec3 vSunShadowWorldNormal;
		struct SunLightShadow {
			float shadowIntensity;
			float shadowBias;
			float shadowNormalBias;
			float shadowRadius;
			vec2 shadowMapSize;
		};
		uniform SunLightShadow sunLightShadows[ NUM_SUN_LIGHT_SHADOWS ];
	#endif
	#if NUM_DIR_LIGHT_SHADOWS > 0
		#if defined( SHADOWMAP_TYPE_PCF )
			uniform sampler2DShadow directionalShadowMap[ NUM_DIR_LIGHT_SHADOWS ];
		#else
			uniform sampler2D directionalShadowMap[ NUM_DIR_LIGHT_SHADOWS ];
		#endif
		varying vec4 vDirectionalShadowCoord[ NUM_DIR_LIGHT_SHADOWS ];
		struct DirectionalLightShadow {
			float shadowIntensity;
			float shadowBias;
			float shadowNormalBias;
			float shadowRadius;
			vec2 shadowMapSize;
		};
		uniform DirectionalLightShadow directionalLightShadows[ NUM_DIR_LIGHT_SHADOWS ];
	#endif
	#if NUM_SPOT_LIGHT_SHADOWS > 0
		#if defined( SHADOWMAP_TYPE_PCF )
			uniform sampler2DShadow spotShadowMap[ NUM_SPOT_LIGHT_SHADOWS ];
		#else
			uniform sampler2D spotShadowMap[ NUM_SPOT_LIGHT_SHADOWS ];
		#endif
		struct SpotLightShadow {
			float shadowIntensity;
			float shadowBias;
			float shadowNormalBias;
			float shadowRadius;
			vec2 shadowMapSize;
		};
		uniform SpotLightShadow spotLightShadows[ NUM_SPOT_LIGHT_SHADOWS ];
	#endif
	#if NUM_POINT_LIGHT_SHADOWS > 0
		#if defined( SHADOWMAP_TYPE_PCF )
			uniform samplerCubeShadow pointShadowMap[ NUM_POINT_LIGHT_SHADOWS ];
		#elif defined( SHADOWMAP_TYPE_BASIC )
			uniform samplerCube pointShadowMap[ NUM_POINT_LIGHT_SHADOWS ];
		#endif
		varying vec4 vPointShadowCoord[ NUM_POINT_LIGHT_SHADOWS ];
		struct PointLightShadow {
			float shadowIntensity;
			float shadowBias;
			float shadowNormalBias;
			float shadowRadius;
			vec2 shadowMapSize;
			float shadowCameraNear;
			float shadowCameraFar;
		};
		uniform PointLightShadow pointLightShadows[ NUM_POINT_LIGHT_SHADOWS ];
	#endif
	#if defined( SHADOWMAP_TYPE_PCF )
		float interleavedGradientNoise( vec2 position ) {
			return fract( 52.9829189 * fract( dot( position, vec2( 0.06711056, 0.00583715 ) ) ) );
		}
		vec2 vogelDiskSample( int sampleIndex, int samplesCount, float phi ) {
			const float goldenAngle = 2.399963229728653;
			float r = sqrt( ( float( sampleIndex ) + 0.5 ) / float( samplesCount ) );
			float theta = float( sampleIndex ) * goldenAngle + phi;
			return vec2( cos( theta ), sin( theta ) ) * r;
		}
	#endif
	#if defined( SHADOWMAP_TYPE_PCF )
		float getShadow( sampler2DShadow shadowMap, vec2 shadowMapSize, float shadowIntensity, float shadowBias, float shadowRadius, vec4 shadowCoord ) {
			float shadow = 1.0;
			shadowCoord.xyz /= shadowCoord.w;
			shadowCoord.z += shadowBias;
			bool inFrustum = shadowCoord.x >= 0.0 && shadowCoord.x <= 1.0 && shadowCoord.y >= 0.0 && shadowCoord.y <= 1.0;
			bool frustumTest = inFrustum && shadowCoord.z <= 1.0;
			if ( frustumTest ) {
				vec2 texelSize = vec2( 1.0 ) / shadowMapSize;
				float radius = shadowRadius * texelSize.x;
				float phi = interleavedGradientNoise( gl_FragCoord.xy ) * PI2;
				shadow = (
					texture( shadowMap, vec3( shadowCoord.xy + vogelDiskSample( 0, 5, phi ) * radius, shadowCoord.z ) ) +
					texture( shadowMap, vec3( shadowCoord.xy + vogelDiskSample( 1, 5, phi ) * radius, shadowCoord.z ) ) +
					texture( shadowMap, vec3( shadowCoord.xy + vogelDiskSample( 2, 5, phi ) * radius, shadowCoord.z ) ) +
					texture( shadowMap, vec3( shadowCoord.xy + vogelDiskSample( 3, 5, phi ) * radius, shadowCoord.z ) ) +
					texture( shadowMap, vec3( shadowCoord.xy + vogelDiskSample( 4, 5, phi ) * radius, shadowCoord.z ) )
				) * 0.2;
			}
			return mix( 1.0, shadow, shadowIntensity );
		}
	#elif defined( SHADOWMAP_TYPE_VSM )
		float getShadow( sampler2D shadowMap, vec2 shadowMapSize, float shadowIntensity, float shadowBias, float shadowRadius, vec4 shadowCoord ) {
			float shadow = 1.0;
			shadowCoord.xyz /= shadowCoord.w;
			#ifdef USE_REVERSED_DEPTH_BUFFER
				shadowCoord.z -= shadowBias;
			#else
				shadowCoord.z += shadowBias;
			#endif
			bool inFrustum = shadowCoord.x >= 0.0 && shadowCoord.x <= 1.0 && shadowCoord.y >= 0.0 && shadowCoord.y <= 1.0;
			bool frustumTest = inFrustum && shadowCoord.z <= 1.0;
			if ( frustumTest ) {
				vec2 distribution = texture2D( shadowMap, shadowCoord.xy ).rg;
				float mean = distribution.x;
				float variance = distribution.y * distribution.y;
				#ifdef USE_REVERSED_DEPTH_BUFFER
					float hard_shadow = step( mean, shadowCoord.z );
				#else
					float hard_shadow = step( shadowCoord.z, mean );
				#endif
				
				if ( hard_shadow == 1.0 ) {
					shadow = 1.0;
				} else {
					variance = max( variance, 0.0000001 );
					float d = shadowCoord.z - mean;
					float p_max = variance / ( variance + d * d );
					p_max = clamp( ( p_max - 0.3 ) / 0.65, 0.0, 1.0 );
					shadow = max( hard_shadow, p_max );
				}
			}
			return mix( 1.0, shadow, shadowIntensity );
		}
	#else
		float getShadow( sampler2D shadowMap, vec2 shadowMapSize, float shadowIntensity, float shadowBias, float shadowRadius, vec4 shadowCoord ) {
			float shadow = 1.0;
			shadowCoord.xyz /= shadowCoord.w;
			#ifdef USE_REVERSED_DEPTH_BUFFER
				shadowCoord.z -= shadowBias;
			#else
				shadowCoord.z += shadowBias;
			#endif
			bool inFrustum = shadowCoord.x >= 0.0 && shadowCoord.x <= 1.0 && shadowCoord.y >= 0.0 && shadowCoord.y <= 1.0;
			bool frustumTest = inFrustum && shadowCoord.z <= 1.0;
			if ( frustumTest ) {
				float depth = texture2D( shadowMap, shadowCoord.xy ).r;
				#ifdef USE_REVERSED_DEPTH_BUFFER
					shadow = step( depth, shadowCoord.z );
				#else
					shadow = step( shadowCoord.z, depth );
				#endif
			}
			return mix( 1.0, shadow, shadowIntensity );
		}
	#endif
	#if NUM_SUN_LIGHT_SHADOWS > 0
		float getSunShadow(
			#if defined( SHADOWMAP_TYPE_PCF )
				sampler2DShadow shadowMap,
			#else
				sampler2D shadowMap,
			#endif
			SunLightShadow sunLightShadow,
			int shadowIndex
		) {
			vec4 shadowWorldPosition = vec4( vSunShadowWorldPosition.xyz + vSunShadowWorldNormal * sunLightShadow.shadowNormalBias, 1.0 );
			float viewDepth = vSunShadowWorldPosition.w;
			int cascadeOffset = shadowIndex * SUN_LIGHT_CASCADES;
			float shadow = 1.0;
			for ( int i = SUN_LIGHT_CASCADES - 1; i >= 0; i -- ) {
				vec4 cascade = sunShadowCascade[ cascadeOffset + i ];
				if ( viewDepth >= cascade.x && viewDepth < cascade.y ) {
					float cascadeShadow = getShadow(
						shadowMap,
						sunLightShadow.shadowMapSize,
						sunLightShadow.shadowIntensity,
						sunLightShadow.shadowBias,
						sunLightShadow.shadowRadius,
						sunShadowMatrix[ cascadeOffset + i ] * shadowWorldPosition
					);
					shadow = mix( cascadeShadow, shadow, smoothstep( cascade.z, cascade.y, viewDepth ) );
				}
			}
			return shadow;
		}
	#endif
	#if NUM_POINT_LIGHT_SHADOWS > 0
	#if defined( SHADOWMAP_TYPE_PCF )
	float getPointShadow( samplerCubeShadow shadowMap, vec2 shadowMapSize, float shadowIntensity, float shadowBias, float shadowRadius, vec4 shadowCoord, float shadowCameraNear, float shadowCameraFar ) {
		float shadow = 1.0;
		vec3 lightToPosition = shadowCoord.xyz;
		vec3 bd3D = normalize( lightToPosition );
		vec3 absVec = abs( lightToPosition );
		float viewSpaceZ = max( max( absVec.x, absVec.y ), absVec.z );
		if ( viewSpaceZ - shadowCameraFar <= 0.0 && viewSpaceZ - shadowCameraNear >= 0.0 ) {
			#ifdef USE_REVERSED_DEPTH_BUFFER
				float dp = ( shadowCameraNear * ( shadowCameraFar - viewSpaceZ ) ) / ( viewSpaceZ * ( shadowCameraFar - shadowCameraNear ) );
				dp -= shadowBias;
			#else
				float dp = ( shadowCameraFar * ( viewSpaceZ - shadowCameraNear ) ) / ( viewSpaceZ * ( shadowCameraFar - shadowCameraNear ) );
				dp += shadowBias;
			#endif
			float texelSize = shadowRadius / shadowMapSize.x;
			vec3 absDir = abs( bd3D );
			vec3 tangent = absDir.x > absDir.z ? vec3( 0.0, 1.0, 0.0 ) : vec3( 1.0, 0.0, 0.0 );
			tangent = normalize( cross( bd3D, tangent ) );
			vec3 bitangent = cross( bd3D, tangent );
			float phi = interleavedGradientNoise( gl_FragCoord.xy ) * PI2;
			vec2 sample0 = vogelDiskSample( 0, 5, phi );
			vec2 sample1 = vogelDiskSample( 1, 5, phi );
			vec2 sample2 = vogelDiskSample( 2, 5, phi );
			vec2 sample3 = vogelDiskSample( 3, 5, phi );
			vec2 sample4 = vogelDiskSample( 4, 5, phi );
			shadow = (
				texture( shadowMap, vec4( bd3D + ( tangent * sample0.x + bitangent * sample0.y ) * texelSize, dp ) ) +
				texture( shadowMap, vec4( bd3D + ( tangent * sample1.x + bitangent * sample1.y ) * texelSize, dp ) ) +
				texture( shadowMap, vec4( bd3D + ( tangent * sample2.x + bitangent * sample2.y ) * texelSize, dp ) ) +
				texture( shadowMap, vec4( bd3D + ( tangent * sample3.x + bitangent * sample3.y ) * texelSize, dp ) ) +
				texture( shadowMap, vec4( bd3D + ( tangent * sample4.x + bitangent * sample4.y ) * texelSize, dp ) )
			) * 0.2;
		}
		return mix( 1.0, shadow, shadowIntensity );
	}
	#elif defined( SHADOWMAP_TYPE_BASIC )
	float getPointShadow( samplerCube shadowMap, vec2 shadowMapSize, float shadowIntensity, float shadowBias, float shadowRadius, vec4 shadowCoord, float shadowCameraNear, float shadowCameraFar ) {
		float shadow = 1.0;
		vec3 lightToPosition = shadowCoord.xyz;
		vec3 absVec = abs( lightToPosition );
		float viewSpaceZ = max( max( absVec.x, absVec.y ), absVec.z );
		if ( viewSpaceZ - shadowCameraFar <= 0.0 && viewSpaceZ - shadowCameraNear >= 0.0 ) {
			float dp = ( shadowCameraFar * ( viewSpaceZ - shadowCameraNear ) ) / ( viewSpaceZ * ( shadowCameraFar - shadowCameraNear ) );
			dp += shadowBias;
			vec3 bd3D = normalize( lightToPosition );
			float depth = textureCube( shadowMap, bd3D ).r;
			#ifdef USE_REVERSED_DEPTH_BUFFER
				depth = 1.0 - depth;
			#endif
			shadow = step( dp, depth );
		}
		return mix( 1.0, shadow, shadowIntensity );
	}
	#endif
	#endif
#endif`,eS=`#if NUM_SPOT_LIGHT_COORDS > 0
	uniform mat4 spotLightMatrix[ NUM_SPOT_LIGHT_COORDS ];
	varying vec4 vSpotLightCoord[ NUM_SPOT_LIGHT_COORDS ];
#endif
#ifdef USE_SHADOWMAP
	#if NUM_SUN_LIGHT_SHADOWS > 0
		varying vec4 vSunShadowWorldPosition;
		varying vec3 vSunShadowWorldNormal;
	#endif
	#if NUM_DIR_LIGHT_SHADOWS > 0
		uniform mat4 directionalShadowMatrix[ NUM_DIR_LIGHT_SHADOWS ];
		varying vec4 vDirectionalShadowCoord[ NUM_DIR_LIGHT_SHADOWS ];
		struct DirectionalLightShadow {
			float shadowIntensity;
			float shadowBias;
			float shadowNormalBias;
			float shadowRadius;
			vec2 shadowMapSize;
		};
		uniform DirectionalLightShadow directionalLightShadows[ NUM_DIR_LIGHT_SHADOWS ];
	#endif
	#if NUM_SPOT_LIGHT_SHADOWS > 0
		struct SpotLightShadow {
			float shadowIntensity;
			float shadowBias;
			float shadowNormalBias;
			float shadowRadius;
			vec2 shadowMapSize;
		};
		uniform SpotLightShadow spotLightShadows[ NUM_SPOT_LIGHT_SHADOWS ];
	#endif
	#if NUM_POINT_LIGHT_SHADOWS > 0
		uniform mat4 pointShadowMatrix[ NUM_POINT_LIGHT_SHADOWS ];
		varying vec4 vPointShadowCoord[ NUM_POINT_LIGHT_SHADOWS ];
		struct PointLightShadow {
			float shadowIntensity;
			float shadowBias;
			float shadowNormalBias;
			float shadowRadius;
			vec2 shadowMapSize;
			float shadowCameraNear;
			float shadowCameraFar;
		};
		uniform PointLightShadow pointLightShadows[ NUM_POINT_LIGHT_SHADOWS ];
	#endif
#endif`,tS=`#if ( defined( USE_SHADOWMAP ) && ( NUM_DIR_LIGHT_SHADOWS > 0 || NUM_SUN_LIGHT_SHADOWS > 0 || NUM_POINT_LIGHT_SHADOWS > 0 ) ) || ( NUM_SPOT_LIGHT_COORDS > 0 )
	#ifdef HAS_NORMAL
		vec3 shadowWorldNormal = transformNormalByInverseViewMatrix( transformedNormal, viewMatrix );
	#else
		vec3 shadowWorldNormal = vec3( 0.0 );
	#endif
	vec4 shadowWorldPosition;
#endif
#if defined( USE_SHADOWMAP )
	#if NUM_SUN_LIGHT_SHADOWS > 0
		vSunShadowWorldPosition = vec4( worldPosition.xyz, - mvPosition.z );
		vSunShadowWorldNormal = shadowWorldNormal;
	#endif
	#if NUM_DIR_LIGHT_SHADOWS > 0
		#pragma unroll_loop_start
		for ( int i = 0; i < NUM_DIR_LIGHT_SHADOWS; i ++ ) {
			shadowWorldPosition = worldPosition + vec4( shadowWorldNormal * directionalLightShadows[ i ].shadowNormalBias, 0 );
			vDirectionalShadowCoord[ i ] = directionalShadowMatrix[ i ] * shadowWorldPosition;
		}
		#pragma unroll_loop_end
	#endif
	#if NUM_POINT_LIGHT_SHADOWS > 0
		#pragma unroll_loop_start
		for ( int i = 0; i < NUM_POINT_LIGHT_SHADOWS; i ++ ) {
			shadowWorldPosition = worldPosition + vec4( shadowWorldNormal * pointLightShadows[ i ].shadowNormalBias, 0 );
			vPointShadowCoord[ i ] = pointShadowMatrix[ i ] * shadowWorldPosition;
		}
		#pragma unroll_loop_end
	#endif
#endif
#if NUM_SPOT_LIGHT_COORDS > 0
	#pragma unroll_loop_start
	for ( int i = 0; i < NUM_SPOT_LIGHT_COORDS; i ++ ) {
		shadowWorldPosition = worldPosition;
		#if ( defined( USE_SHADOWMAP ) && UNROLLED_LOOP_INDEX < NUM_SPOT_LIGHT_SHADOWS )
			shadowWorldPosition.xyz += shadowWorldNormal * spotLightShadows[ i ].shadowNormalBias;
		#endif
		vSpotLightCoord[ i ] = spotLightMatrix[ i ] * shadowWorldPosition;
	}
	#pragma unroll_loop_end
#endif`,nS=`float getShadowMask() {
	float shadow = 1.0;
	#ifdef USE_SHADOWMAP
	#if NUM_SUN_LIGHT_SHADOWS > 0
	SunLightShadow sunLight;
	#pragma unroll_loop_start
	for ( int i = 0; i < NUM_SUN_LIGHT_SHADOWS; i ++ ) {
		sunLight = sunLightShadows[ i ];
		shadow *= receiveShadow ? getSunShadow( sunShadowMap[ i ], sunLight, UNROLLED_LOOP_INDEX ) : 1.0;
	}
	#pragma unroll_loop_end
	#endif
	#if NUM_DIR_LIGHT_SHADOWS > 0
	DirectionalLightShadow directionalLight;
	#pragma unroll_loop_start
	for ( int i = 0; i < NUM_DIR_LIGHT_SHADOWS; i ++ ) {
		directionalLight = directionalLightShadows[ i ];
		shadow *= receiveShadow ? getShadow( directionalShadowMap[ i ], directionalLight.shadowMapSize, directionalLight.shadowIntensity, directionalLight.shadowBias, directionalLight.shadowRadius, vDirectionalShadowCoord[ i ] ) : 1.0;
	}
	#pragma unroll_loop_end
	#endif
	#if NUM_SPOT_LIGHT_SHADOWS > 0
	SpotLightShadow spotLight;
	#pragma unroll_loop_start
	for ( int i = 0; i < NUM_SPOT_LIGHT_SHADOWS; i ++ ) {
		spotLight = spotLightShadows[ i ];
		shadow *= receiveShadow ? getShadow( spotShadowMap[ i ], spotLight.shadowMapSize, spotLight.shadowIntensity, spotLight.shadowBias, spotLight.shadowRadius, vSpotLightCoord[ i ] ) : 1.0;
	}
	#pragma unroll_loop_end
	#endif
	#if NUM_POINT_LIGHT_SHADOWS > 0 && ( defined( SHADOWMAP_TYPE_PCF ) || defined( SHADOWMAP_TYPE_BASIC ) )
	PointLightShadow pointLight;
	#pragma unroll_loop_start
	for ( int i = 0; i < NUM_POINT_LIGHT_SHADOWS; i ++ ) {
		pointLight = pointLightShadows[ i ];
		shadow *= receiveShadow ? getPointShadow( pointShadowMap[ i ], pointLight.shadowMapSize, pointLight.shadowIntensity, pointLight.shadowBias, pointLight.shadowRadius, vPointShadowCoord[ i ], pointLight.shadowCameraNear, pointLight.shadowCameraFar ) : 1.0;
	}
	#pragma unroll_loop_end
	#endif
	#endif
	return shadow;
}`,iS=`#ifdef USE_SKINNING
	mat4 boneMatX = getBoneMatrix( skinIndex.x );
	mat4 boneMatY = getBoneMatrix( skinIndex.y );
	mat4 boneMatZ = getBoneMatrix( skinIndex.z );
	mat4 boneMatW = getBoneMatrix( skinIndex.w );
#endif`,sS=`#ifdef USE_SKINNING
	uniform mat4 bindMatrix;
	uniform mat4 bindMatrixInverse;
	uniform highp sampler2D boneTexture;
	mat4 getBoneMatrix( const in float i ) {
		int size = textureSize( boneTexture, 0 ).x;
		int j = int( i ) * 4;
		int x = j % size;
		int y = j / size;
		vec4 v1 = texelFetch( boneTexture, ivec2( x, y ), 0 );
		vec4 v2 = texelFetch( boneTexture, ivec2( x + 1, y ), 0 );
		vec4 v3 = texelFetch( boneTexture, ivec2( x + 2, y ), 0 );
		vec4 v4 = texelFetch( boneTexture, ivec2( x + 3, y ), 0 );
		return mat4( v1, v2, v3, v4 );
	}
#endif`,rS=`#ifdef USE_SKINNING
	vec4 skinVertex = bindMatrix * vec4( transformed, 1.0 );
	vec4 skinned = vec4( 0.0 );
	skinned += boneMatX * skinVertex * skinWeight.x;
	skinned += boneMatY * skinVertex * skinWeight.y;
	skinned += boneMatZ * skinVertex * skinWeight.z;
	skinned += boneMatW * skinVertex * skinWeight.w;
	transformed = ( bindMatrixInverse * skinned ).xyz;
#endif`,oS=`#ifdef USE_SKINNING
	mat4 skinMatrix = mat4( 0.0 );
	skinMatrix += skinWeight.x * boneMatX;
	skinMatrix += skinWeight.y * boneMatY;
	skinMatrix += skinWeight.z * boneMatZ;
	skinMatrix += skinWeight.w * boneMatW;
	skinMatrix = bindMatrixInverse * skinMatrix * bindMatrix;
	objectNormal = vec4( skinMatrix * vec4( objectNormal, 0.0 ) ).xyz;
	#ifdef USE_TANGENT
		objectTangent = vec4( skinMatrix * vec4( objectTangent, 0.0 ) ).xyz;
	#endif
#endif`,aS=`float specularStrength;
#ifdef USE_SPECULARMAP
	vec4 texelSpecular = texture2D( specularMap, vSpecularMapUv );
	specularStrength = texelSpecular.r;
#else
	specularStrength = 1.0;
#endif`,lS=`#ifdef USE_SPECULARMAP
	uniform sampler2D specularMap;
#endif`,cS=`#if defined( TONE_MAPPING )
	gl_FragColor.rgb = toneMapping( gl_FragColor.rgb );
#endif`,uS=`#ifndef saturate
#define saturate( a ) clamp( a, 0.0, 1.0 )
#endif
uniform float toneMappingExposure;
vec3 LinearToneMapping( vec3 color ) {
	return saturate( toneMappingExposure * color );
}
vec3 ReinhardToneMapping( vec3 color ) {
	color *= toneMappingExposure;
	return saturate( color / ( vec3( 1.0 ) + color ) );
}
vec3 CineonToneMapping( vec3 color ) {
	color *= toneMappingExposure;
	color = max( vec3( 0.0 ), color - 0.004 );
	return pow( ( color * ( 6.2 * color + 0.5 ) ) / ( color * ( 6.2 * color + 1.7 ) + 0.06 ), vec3( 2.2 ) );
}
vec3 RRTAndODTFit( vec3 v ) {
	vec3 a = v * ( v + 0.0245786 ) - 0.000090537;
	vec3 b = v * ( 0.983729 * v + 0.4329510 ) + 0.238081;
	return a / b;
}
vec3 ACESFilmicToneMapping( vec3 color ) {
	const mat3 ACESInputMat = mat3(
		vec3( 0.59719, 0.07600, 0.02840 ),		vec3( 0.35458, 0.90834, 0.13383 ),
		vec3( 0.04823, 0.01566, 0.83777 )
	);
	const mat3 ACESOutputMat = mat3(
		vec3(  1.60475, -0.10208, -0.00327 ),		vec3( -0.53108,  1.10813, -0.07276 ),
		vec3( -0.07367, -0.00605,  1.07602 )
	);
	color *= toneMappingExposure / 0.6;
	color = ACESInputMat * color;
	color = RRTAndODTFit( color );
	color = ACESOutputMat * color;
	return saturate( color );
}
const mat3 LINEAR_REC2020_TO_LINEAR_SRGB = mat3(
	vec3( 1.6605, - 0.1246, - 0.0182 ),
	vec3( - 0.5876, 1.1329, - 0.1006 ),
	vec3( - 0.0728, - 0.0083, 1.1187 )
);
const mat3 LINEAR_SRGB_TO_LINEAR_REC2020 = mat3(
	vec3( 0.6274, 0.0691, 0.0164 ),
	vec3( 0.3293, 0.9195, 0.0880 ),
	vec3( 0.0433, 0.0113, 0.8956 )
);
vec3 agxDefaultContrastApprox( vec3 x ) {
	vec3 x2 = x * x;
	vec3 x4 = x2 * x2;
	return + 15.5 * x4 * x2
		- 40.14 * x4 * x
		+ 31.96 * x4
		- 6.868 * x2 * x
		+ 0.4298 * x2
		+ 0.1191 * x
		- 0.00232;
}
vec3 AgXToneMapping( vec3 color ) {
	const mat3 AgXInsetMatrix = mat3(
		vec3( 0.856627153315983, 0.137318972929847, 0.11189821299995 ),
		vec3( 0.0951212405381588, 0.761241990602591, 0.0767994186031903 ),
		vec3( 0.0482516061458583, 0.101439036467562, 0.811302368396859 )
	);
	const mat3 AgXOutsetMatrix = mat3(
		vec3( 1.1271005818144368, - 0.1413297634984383, - 0.14132976349843826 ),
		vec3( - 0.11060664309660323, 1.157823702216272, - 0.11060664309660294 ),
		vec3( - 0.016493938717834573, - 0.016493938717834257, 1.2519364065950405 )
	);
	const float AgxMinEv = - 12.47393;	const float AgxMaxEv = 4.026069;
	color *= toneMappingExposure;
	color = LINEAR_SRGB_TO_LINEAR_REC2020 * color;
	color = AgXInsetMatrix * color;
	color = max( color, 1e-10 );	color = log2( color );
	color = ( color - AgxMinEv ) / ( AgxMaxEv - AgxMinEv );
	color = clamp( color, 0.0, 1.0 );
	color = agxDefaultContrastApprox( color );
	color = AgXOutsetMatrix * color;
	color = pow( max( vec3( 0.0 ), color ), vec3( 2.2 ) );
	color = LINEAR_REC2020_TO_LINEAR_SRGB * color;
	color = clamp( color, 0.0, 1.0 );
	return color;
}
vec3 NeutralToneMapping( vec3 color ) {
	const float StartCompression = 0.8 - 0.04;
	const float Desaturation = 0.15;
	color *= toneMappingExposure;
	float x = min( color.r, min( color.g, color.b ) );
	float offset = x < 0.08 ? x - 6.25 * x * x : 0.04;
	color -= offset;
	float peak = max( color.r, max( color.g, color.b ) );
	if ( peak < StartCompression ) return color;
	float d = 1. - StartCompression;
	float newPeak = 1. - d * d / ( peak + d - StartCompression );
	color *= newPeak / peak;
	float g = 1. - 1. / ( Desaturation * ( peak - newPeak ) + 1. );
	return mix( color, vec3( newPeak ), g );
}
vec3 CustomToneMapping( vec3 color ) { return color; }`,hS=`#ifdef USE_TRANSMISSION
	material.transmission = transmission;
	material.transmissionAlpha = 1.0;
	material.thickness = thickness;
	material.attenuationDistance = attenuationDistance;
	material.attenuationColor = attenuationColor;
	#ifdef USE_TRANSMISSIONMAP
		material.transmission *= texture2D( transmissionMap, vTransmissionMapUv ).r;
	#endif
	#ifdef USE_THICKNESSMAP
		material.thickness *= texture2D( thicknessMap, vThicknessMapUv ).g;
	#endif
	vec3 pos = vWorldPosition;
	vec3 v = normalize( cameraPosition - pos );
	vec3 n = transformNormalByInverseViewMatrix( normal, viewMatrix );
	vec4 transmitted = getIBLVolumeRefraction(
		n, v, material.roughness, material.diffuseContribution, material.specularColorBlended, material.specularF90,
		pos, modelMatrix, viewMatrix, projectionMatrix, material.dispersion, material.ior, material.thickness,
		material.attenuationColor, material.attenuationDistance );
	material.transmissionAlpha = mix( material.transmissionAlpha, transmitted.a, material.transmission );
	totalDiffuse = mix( totalDiffuse, transmitted.rgb, material.transmission );
#endif`,dS=`#ifdef USE_TRANSMISSION
	uniform float transmission;
	uniform float thickness;
	uniform float attenuationDistance;
	uniform vec3 attenuationColor;
	#ifdef USE_TRANSMISSIONMAP
		uniform sampler2D transmissionMap;
	#endif
	#ifdef USE_THICKNESSMAP
		uniform sampler2D thicknessMap;
	#endif
	uniform vec2 transmissionSamplerSize;
	uniform sampler2D transmissionSamplerMap;
	uniform mat4 modelMatrix;
	uniform mat4 projectionMatrix;
	varying vec3 vWorldPosition;
	float w0( float a ) {
		return ( 1.0 / 6.0 ) * ( a * ( a * ( - a + 3.0 ) - 3.0 ) + 1.0 );
	}
	float w1( float a ) {
		return ( 1.0 / 6.0 ) * ( a *  a * ( 3.0 * a - 6.0 ) + 4.0 );
	}
	float w2( float a ){
		return ( 1.0 / 6.0 ) * ( a * ( a * ( - 3.0 * a + 3.0 ) + 3.0 ) + 1.0 );
	}
	float w3( float a ) {
		return ( 1.0 / 6.0 ) * ( a * a * a );
	}
	float g0( float a ) {
		return w0( a ) + w1( a );
	}
	float g1( float a ) {
		return w2( a ) + w3( a );
	}
	float h0( float a ) {
		return - 1.0 + w1( a ) / ( w0( a ) + w1( a ) );
	}
	float h1( float a ) {
		return 1.0 + w3( a ) / ( w2( a ) + w3( a ) );
	}
	vec4 bicubic( sampler2D tex, vec2 uv, vec4 texelSize, float lod ) {
		uv = uv * texelSize.zw + 0.5;
		vec2 iuv = floor( uv );
		vec2 fuv = fract( uv );
		float g0x = g0( fuv.x );
		float g1x = g1( fuv.x );
		float h0x = h0( fuv.x );
		float h1x = h1( fuv.x );
		float h0y = h0( fuv.y );
		float h1y = h1( fuv.y );
		vec2 p0 = ( vec2( iuv.x + h0x, iuv.y + h0y ) - 0.5 ) * texelSize.xy;
		vec2 p1 = ( vec2( iuv.x + h1x, iuv.y + h0y ) - 0.5 ) * texelSize.xy;
		vec2 p2 = ( vec2( iuv.x + h0x, iuv.y + h1y ) - 0.5 ) * texelSize.xy;
		vec2 p3 = ( vec2( iuv.x + h1x, iuv.y + h1y ) - 0.5 ) * texelSize.xy;
		return g0( fuv.y ) * ( g0x * textureLod( tex, p0, lod ) + g1x * textureLod( tex, p1, lod ) ) +
			g1( fuv.y ) * ( g0x * textureLod( tex, p2, lod ) + g1x * textureLod( tex, p3, lod ) );
	}
	vec4 textureBicubic( sampler2D sampler, vec2 uv, float lod ) {
		vec2 fLodSize = vec2( textureSize( sampler, int( lod ) ) );
		vec2 cLodSize = vec2( textureSize( sampler, int( lod + 1.0 ) ) );
		vec2 fLodSizeInv = 1.0 / fLodSize;
		vec2 cLodSizeInv = 1.0 / cLodSize;
		vec4 fSample = bicubic( sampler, uv, vec4( fLodSizeInv, fLodSize ), floor( lod ) );
		vec4 cSample = bicubic( sampler, uv, vec4( cLodSizeInv, cLodSize ), ceil( lod ) );
		return mix( fSample, cSample, fract( lod ) );
	}
	vec3 getVolumeTransmissionRay( const in vec3 n, const in vec3 v, const in float thickness, const in float ior, const in mat4 modelMatrix ) {
		vec3 refractionVector = refract( - v, normalize( n ), 1.0 / ior );
		vec3 modelScale;
		modelScale.x = length( vec3( modelMatrix[ 0 ].xyz ) );
		modelScale.y = length( vec3( modelMatrix[ 1 ].xyz ) );
		modelScale.z = length( vec3( modelMatrix[ 2 ].xyz ) );
		return normalize( refractionVector ) * thickness * modelScale;
	}
	float applyIorToRoughness( const in float roughness, const in float ior ) {
		return roughness * clamp( ior * 2.0 - 2.0, 0.0, 1.0 );
	}
	vec4 getTransmissionSample( const in vec2 fragCoord, const in float roughness, const in float ior ) {
		float lod = log2( transmissionSamplerSize.x ) * applyIorToRoughness( roughness, ior );
		return textureBicubic( transmissionSamplerMap, fragCoord.xy, lod );
	}
	vec3 volumeAttenuation( const in float transmissionDistance, const in vec3 attenuationColor, const in float attenuationDistance ) {
		if ( isinf( attenuationDistance ) ) {
			return vec3( 1.0 );
		} else {
			vec3 attenuationCoefficient = -log( attenuationColor ) / attenuationDistance;
			vec3 transmittance = exp( - attenuationCoefficient * transmissionDistance );			return transmittance;
		}
	}
	vec4 getIBLVolumeRefraction( const in vec3 n, const in vec3 v, const in float roughness, const in vec3 diffuseColor,
		const in vec3 specularColor, const in float specularF90, const in vec3 position, const in mat4 modelMatrix,
		const in mat4 viewMatrix, const in mat4 projMatrix, const in float dispersion, const in float ior, const in float thickness,
		const in vec3 attenuationColor, const in float attenuationDistance ) {
		vec4 transmittedLight;
		vec3 transmittance;
		#ifdef USE_DISPERSION
			float halfSpread = ( ior - 1.0 ) * 0.025 * dispersion;
			vec3 iors = vec3( ior - halfSpread, ior, ior + halfSpread );
			for ( int i = 0; i < 3; i ++ ) {
				vec3 transmissionRay = getVolumeTransmissionRay( n, v, thickness, iors[ i ], modelMatrix );
				vec3 refractedRayExit = position + transmissionRay;
				vec4 ndcPos = projMatrix * viewMatrix * vec4( refractedRayExit, 1.0 );
				vec2 refractionCoords = ndcPos.xy / ndcPos.w;
				refractionCoords += 1.0;
				refractionCoords /= 2.0;
				vec4 transmissionSample = getTransmissionSample( refractionCoords, roughness, iors[ i ] );
				transmittedLight[ i ] = transmissionSample[ i ];
				transmittedLight.a += transmissionSample.a;
				transmittance[ i ] = diffuseColor[ i ] * volumeAttenuation( length( transmissionRay ), attenuationColor, attenuationDistance )[ i ];
			}
			transmittedLight.a /= 3.0;
		#else
			vec3 transmissionRay = getVolumeTransmissionRay( n, v, thickness, ior, modelMatrix );
			vec3 refractedRayExit = position + transmissionRay;
			vec4 ndcPos = projMatrix * viewMatrix * vec4( refractedRayExit, 1.0 );
			vec2 refractionCoords = ndcPos.xy / ndcPos.w;
			refractionCoords += 1.0;
			refractionCoords /= 2.0;
			transmittedLight = getTransmissionSample( refractionCoords, roughness, ior );
			transmittance = diffuseColor * volumeAttenuation( length( transmissionRay ), attenuationColor, attenuationDistance );
		#endif
		vec3 attenuatedColor = transmittance * transmittedLight.rgb;
		vec3 F = EnvironmentBRDF( n, v, specularColor, specularF90, roughness );
		float transmittanceFactor = ( transmittance.r + transmittance.g + transmittance.b ) / 3.0;
		return vec4( ( 1.0 - F ) * attenuatedColor, 1.0 - ( 1.0 - transmittedLight.a ) * transmittanceFactor );
	}
#endif`,fS=`#if defined( USE_UV ) || defined( USE_ANISOTROPY )
	varying vec2 vUv;
#endif
#ifdef USE_MAP
	varying vec2 vMapUv;
#endif
#ifdef USE_ALPHAMAP
	varying vec2 vAlphaMapUv;
#endif
#ifdef USE_LIGHTMAP
	varying vec2 vLightMapUv;
#endif
#ifdef USE_AOMAP
	varying vec2 vAoMapUv;
#endif
#ifdef USE_BUMPMAP
	varying vec2 vBumpMapUv;
#endif
#ifdef USE_NORMALMAP
	varying vec2 vNormalMapUv;
#endif
#ifdef USE_EMISSIVEMAP
	varying vec2 vEmissiveMapUv;
#endif
#ifdef USE_METALNESSMAP
	varying vec2 vMetalnessMapUv;
#endif
#ifdef USE_ROUGHNESSMAP
	varying vec2 vRoughnessMapUv;
#endif
#ifdef USE_ANISOTROPYMAP
	varying vec2 vAnisotropyMapUv;
#endif
#ifdef USE_CLEARCOATMAP
	varying vec2 vClearcoatMapUv;
#endif
#ifdef USE_CLEARCOAT_NORMALMAP
	varying vec2 vClearcoatNormalMapUv;
#endif
#ifdef USE_CLEARCOAT_ROUGHNESSMAP
	varying vec2 vClearcoatRoughnessMapUv;
#endif
#ifdef USE_IRIDESCENCEMAP
	varying vec2 vIridescenceMapUv;
#endif
#ifdef USE_IRIDESCENCE_THICKNESSMAP
	varying vec2 vIridescenceThicknessMapUv;
#endif
#ifdef USE_SHEEN_COLORMAP
	varying vec2 vSheenColorMapUv;
#endif
#ifdef USE_SHEEN_ROUGHNESSMAP
	varying vec2 vSheenRoughnessMapUv;
#endif
#ifdef USE_SPECULARMAP
	varying vec2 vSpecularMapUv;
#endif
#ifdef USE_SPECULAR_COLORMAP
	varying vec2 vSpecularColorMapUv;
#endif
#ifdef USE_SPECULAR_INTENSITYMAP
	varying vec2 vSpecularIntensityMapUv;
#endif
#ifdef USE_TRANSMISSIONMAP
	uniform mat3 transmissionMapTransform;
	varying vec2 vTransmissionMapUv;
#endif
#ifdef USE_THICKNESSMAP
	uniform mat3 thicknessMapTransform;
	varying vec2 vThicknessMapUv;
#endif`,pS=`#if defined( USE_UV ) || defined( USE_ANISOTROPY )
	varying vec2 vUv;
#endif
#ifdef USE_MAP
	uniform mat3 mapTransform;
	varying vec2 vMapUv;
#endif
#ifdef USE_ALPHAMAP
	uniform mat3 alphaMapTransform;
	varying vec2 vAlphaMapUv;
#endif
#ifdef USE_LIGHTMAP
	uniform mat3 lightMapTransform;
	varying vec2 vLightMapUv;
#endif
#ifdef USE_AOMAP
	uniform mat3 aoMapTransform;
	varying vec2 vAoMapUv;
#endif
#ifdef USE_BUMPMAP
	uniform mat3 bumpMapTransform;
	varying vec2 vBumpMapUv;
#endif
#ifdef USE_NORMALMAP
	uniform mat3 normalMapTransform;
	varying vec2 vNormalMapUv;
#endif
#ifdef USE_DISPLACEMENTMAP
	uniform mat3 displacementMapTransform;
	varying vec2 vDisplacementMapUv;
#endif
#ifdef USE_EMISSIVEMAP
	uniform mat3 emissiveMapTransform;
	varying vec2 vEmissiveMapUv;
#endif
#ifdef USE_METALNESSMAP
	uniform mat3 metalnessMapTransform;
	varying vec2 vMetalnessMapUv;
#endif
#ifdef USE_ROUGHNESSMAP
	uniform mat3 roughnessMapTransform;
	varying vec2 vRoughnessMapUv;
#endif
#ifdef USE_ANISOTROPYMAP
	uniform mat3 anisotropyMapTransform;
	varying vec2 vAnisotropyMapUv;
#endif
#ifdef USE_CLEARCOATMAP
	uniform mat3 clearcoatMapTransform;
	varying vec2 vClearcoatMapUv;
#endif
#ifdef USE_CLEARCOAT_NORMALMAP
	uniform mat3 clearcoatNormalMapTransform;
	varying vec2 vClearcoatNormalMapUv;
#endif
#ifdef USE_CLEARCOAT_ROUGHNESSMAP
	uniform mat3 clearcoatRoughnessMapTransform;
	varying vec2 vClearcoatRoughnessMapUv;
#endif
#ifdef USE_SHEEN_COLORMAP
	uniform mat3 sheenColorMapTransform;
	varying vec2 vSheenColorMapUv;
#endif
#ifdef USE_SHEEN_ROUGHNESSMAP
	uniform mat3 sheenRoughnessMapTransform;
	varying vec2 vSheenRoughnessMapUv;
#endif
#ifdef USE_IRIDESCENCEMAP
	uniform mat3 iridescenceMapTransform;
	varying vec2 vIridescenceMapUv;
#endif
#ifdef USE_IRIDESCENCE_THICKNESSMAP
	uniform mat3 iridescenceThicknessMapTransform;
	varying vec2 vIridescenceThicknessMapUv;
#endif
#ifdef USE_SPECULARMAP
	uniform mat3 specularMapTransform;
	varying vec2 vSpecularMapUv;
#endif
#ifdef USE_SPECULAR_COLORMAP
	uniform mat3 specularColorMapTransform;
	varying vec2 vSpecularColorMapUv;
#endif
#ifdef USE_SPECULAR_INTENSITYMAP
	uniform mat3 specularIntensityMapTransform;
	varying vec2 vSpecularIntensityMapUv;
#endif
#ifdef USE_TRANSMISSIONMAP
	uniform mat3 transmissionMapTransform;
	varying vec2 vTransmissionMapUv;
#endif
#ifdef USE_THICKNESSMAP
	uniform mat3 thicknessMapTransform;
	varying vec2 vThicknessMapUv;
#endif`,mS=`#if defined( USE_UV ) || defined( USE_ANISOTROPY )
	vUv = vec3( uv, 1 ).xy;
#endif
#ifdef USE_MAP
	vMapUv = ( mapTransform * vec3( MAP_UV, 1 ) ).xy;
#endif
#ifdef USE_ALPHAMAP
	vAlphaMapUv = ( alphaMapTransform * vec3( ALPHAMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_LIGHTMAP
	vLightMapUv = ( lightMapTransform * vec3( LIGHTMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_AOMAP
	vAoMapUv = ( aoMapTransform * vec3( AOMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_BUMPMAP
	vBumpMapUv = ( bumpMapTransform * vec3( BUMPMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_NORMALMAP
	vNormalMapUv = ( normalMapTransform * vec3( NORMALMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_DISPLACEMENTMAP
	vDisplacementMapUv = ( displacementMapTransform * vec3( DISPLACEMENTMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_EMISSIVEMAP
	vEmissiveMapUv = ( emissiveMapTransform * vec3( EMISSIVEMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_METALNESSMAP
	vMetalnessMapUv = ( metalnessMapTransform * vec3( METALNESSMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_ROUGHNESSMAP
	vRoughnessMapUv = ( roughnessMapTransform * vec3( ROUGHNESSMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_ANISOTROPYMAP
	vAnisotropyMapUv = ( anisotropyMapTransform * vec3( ANISOTROPYMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_CLEARCOATMAP
	vClearcoatMapUv = ( clearcoatMapTransform * vec3( CLEARCOATMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_CLEARCOAT_NORMALMAP
	vClearcoatNormalMapUv = ( clearcoatNormalMapTransform * vec3( CLEARCOAT_NORMALMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_CLEARCOAT_ROUGHNESSMAP
	vClearcoatRoughnessMapUv = ( clearcoatRoughnessMapTransform * vec3( CLEARCOAT_ROUGHNESSMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_IRIDESCENCEMAP
	vIridescenceMapUv = ( iridescenceMapTransform * vec3( IRIDESCENCEMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_IRIDESCENCE_THICKNESSMAP
	vIridescenceThicknessMapUv = ( iridescenceThicknessMapTransform * vec3( IRIDESCENCE_THICKNESSMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_SHEEN_COLORMAP
	vSheenColorMapUv = ( sheenColorMapTransform * vec3( SHEEN_COLORMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_SHEEN_ROUGHNESSMAP
	vSheenRoughnessMapUv = ( sheenRoughnessMapTransform * vec3( SHEEN_ROUGHNESSMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_SPECULARMAP
	vSpecularMapUv = ( specularMapTransform * vec3( SPECULARMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_SPECULAR_COLORMAP
	vSpecularColorMapUv = ( specularColorMapTransform * vec3( SPECULAR_COLORMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_SPECULAR_INTENSITYMAP
	vSpecularIntensityMapUv = ( specularIntensityMapTransform * vec3( SPECULAR_INTENSITYMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_TRANSMISSIONMAP
	vTransmissionMapUv = ( transmissionMapTransform * vec3( TRANSMISSIONMAP_UV, 1 ) ).xy;
#endif
#ifdef USE_THICKNESSMAP
	vThicknessMapUv = ( thicknessMapTransform * vec3( THICKNESSMAP_UV, 1 ) ).xy;
#endif`,gS=`#if defined( USE_ENVMAP ) || defined( DISTANCE ) || defined ( USE_SHADOWMAP ) || defined ( USE_TRANSMISSION ) || NUM_SPOT_LIGHT_COORDS > 0
	vec4 worldPosition = vec4( transformed, 1.0 );
	#ifdef USE_BATCHING
		worldPosition = batchingMatrix * worldPosition;
	#endif
	#ifdef USE_INSTANCING
		worldPosition = instanceMatrix * worldPosition;
	#endif
	worldPosition = modelMatrix * worldPosition;
#endif`,xS=`varying vec2 vUv;
uniform mat3 uvTransform;
void main() {
	vUv = ( uvTransform * vec3( uv, 1 ) ).xy;
	gl_Position = vec4( position.xy, 1.0, 1.0 );
}`,yS=`uniform sampler2D t2D;
uniform float backgroundIntensity;
varying vec2 vUv;
void main() {
	vec4 texColor = texture2D( t2D, vUv );
	#ifdef DECODE_VIDEO_TEXTURE
		texColor = vec4( mix( pow( texColor.rgb * 0.9478672986 + vec3( 0.0521327014 ), vec3( 2.4 ) ), texColor.rgb * 0.0773993808, vec3( lessThanEqual( texColor.rgb, vec3( 0.04045 ) ) ) ), texColor.w );
	#endif
	texColor.rgb *= backgroundIntensity;
	gl_FragColor = texColor;
	#include <tonemapping_fragment>
	#include <colorspace_fragment>
}`,_S=`varying vec3 vWorldDirection;
#include <common>
void main() {
	vWorldDirection = transformDirection( position, modelMatrix );
	#include <begin_vertex>
	#include <project_vertex>
	gl_Position.z = gl_Position.w;
}`,bS=`#ifdef ENVMAP_TYPE_CUBE
	uniform samplerCube envMap;
#elif defined( ENVMAP_TYPE_CUBE_UV )
	uniform sampler2D envMap;
#endif
uniform float backgroundBlurriness;
uniform float backgroundIntensity;
uniform mat3 backgroundRotation;
varying vec3 vWorldDirection;
#include <cube_uv_reflection_fragment>
void main() {
	#ifdef ENVMAP_TYPE_CUBE
		vec4 texColor = textureCube( envMap, backgroundRotation * vWorldDirection );
	#elif defined( ENVMAP_TYPE_CUBE_UV )
		vec4 texColor = textureCubeUV( envMap, backgroundRotation * vWorldDirection, backgroundBlurriness );
	#else
		vec4 texColor = vec4( 0.0, 0.0, 0.0, 1.0 );
	#endif
	texColor.rgb *= backgroundIntensity;
	gl_FragColor = texColor;
	#include <tonemapping_fragment>
	#include <colorspace_fragment>
}`,SS=`varying vec3 vWorldDirection;
#include <common>
void main() {
	vWorldDirection = transformDirection( position, modelMatrix );
	#include <begin_vertex>
	#include <project_vertex>
	gl_Position.z = gl_Position.w;
}`,vS=`uniform samplerCube tCube;
uniform float tFlip;
uniform float opacity;
varying vec3 vWorldDirection;
void main() {
	vec4 texColor = textureCube( tCube, vec3( tFlip * vWorldDirection.x, vWorldDirection.yz ) );
	gl_FragColor = texColor;
	gl_FragColor.a *= opacity;
	#include <tonemapping_fragment>
	#include <colorspace_fragment>
}`,AS=`#include <common>
#include <batching_pars_vertex>
#include <uv_pars_vertex>
#include <displacementmap_pars_vertex>
#include <morphtarget_pars_vertex>
#include <skinning_pars_vertex>
#include <logdepthbuf_pars_vertex>
#include <clipping_planes_pars_vertex>
varying vec2 vHighPrecisionZW;
void main() {
	#include <uv_vertex>
	#include <batching_vertex>
	#include <skinbase_vertex>
	#include <morphinstance_vertex>
	#ifdef USE_DISPLACEMENTMAP
		#include <beginnormal_vertex>
		#include <morphnormal_vertex>
		#include <skinnormal_vertex>
	#endif
	#include <begin_vertex>
	#include <morphtarget_vertex>
	#include <skinning_vertex>
	#include <displacementmap_vertex>
	#include <project_vertex>
	#include <logdepthbuf_vertex>
	#include <clipping_planes_vertex>
	vHighPrecisionZW = gl_Position.zw;
}`,TS=`#if DEPTH_PACKING == 3200
	uniform float opacity;
#endif
#include <common>
#include <packing>
#include <uv_pars_fragment>
#include <map_pars_fragment>
#include <alphamap_pars_fragment>
#include <alphatest_pars_fragment>
#include <alphahash_pars_fragment>
#include <logdepthbuf_pars_fragment>
#include <clipping_planes_pars_fragment>
varying vec2 vHighPrecisionZW;
void main() {
	vec4 diffuseColor = vec4( 1.0 );
	#include <clipping_planes_fragment>
	#if DEPTH_PACKING == 3200
		diffuseColor.a = opacity;
	#endif
	#include <map_fragment>
	#include <alphamap_fragment>
	#include <alphatest_fragment>
	#include <alphahash_fragment>
	#include <logdepthbuf_fragment>
	#ifdef USE_REVERSED_DEPTH_BUFFER
		float fragCoordZ = vHighPrecisionZW[ 0 ] / vHighPrecisionZW[ 1 ];
	#else
		float fragCoordZ = 0.5 * vHighPrecisionZW[ 0 ] / vHighPrecisionZW[ 1 ] + 0.5;
	#endif
	#if DEPTH_PACKING == 3200
		gl_FragColor = vec4( vec3( 1.0 - fragCoordZ ), opacity );
	#elif DEPTH_PACKING == 3201
		gl_FragColor = packDepthToRGBA( fragCoordZ );
	#elif DEPTH_PACKING == 3202
		gl_FragColor = vec4( packDepthToRGB( fragCoordZ ), 1.0 );
	#elif DEPTH_PACKING == 3203
		gl_FragColor = vec4( packDepthToRG( fragCoordZ ), 0.0, 1.0 );
	#endif
}`,ES=`#define DISTANCE
varying vec3 vWorldPosition;
#include <common>
#include <batching_pars_vertex>
#include <uv_pars_vertex>
#include <displacementmap_pars_vertex>
#include <morphtarget_pars_vertex>
#include <skinning_pars_vertex>
#include <clipping_planes_pars_vertex>
void main() {
	#include <uv_vertex>
	#include <batching_vertex>
	#include <skinbase_vertex>
	#include <morphinstance_vertex>
	#ifdef USE_DISPLACEMENTMAP
		#include <beginnormal_vertex>
		#include <morphnormal_vertex>
		#include <skinnormal_vertex>
	#endif
	#include <begin_vertex>
	#include <morphtarget_vertex>
	#include <skinning_vertex>
	#include <displacementmap_vertex>
	#include <project_vertex>
	#include <worldpos_vertex>
	#include <clipping_planes_vertex>
	vWorldPosition = worldPosition.xyz;
}`,MS=`#define DISTANCE
uniform vec3 referencePosition;
uniform float nearDistance;
uniform float farDistance;
varying vec3 vWorldPosition;
#include <common>
#include <uv_pars_fragment>
#include <map_pars_fragment>
#include <alphamap_pars_fragment>
#include <alphatest_pars_fragment>
#include <alphahash_pars_fragment>
#include <clipping_planes_pars_fragment>
void main() {
	vec4 diffuseColor = vec4( 1.0 );
	#include <clipping_planes_fragment>
	#include <map_fragment>
	#include <alphamap_fragment>
	#include <alphatest_fragment>
	#include <alphahash_fragment>
	float dist = length( vWorldPosition - referencePosition );
	dist = ( dist - nearDistance ) / ( farDistance - nearDistance );
	dist = saturate( dist );
	gl_FragColor = vec4( dist, 0.0, 0.0, 1.0 );
}`,RS=`varying vec3 vWorldDirection;
#include <common>
void main() {
	vWorldDirection = transformDirection( position, modelMatrix );
	#include <begin_vertex>
	#include <project_vertex>
}`,wS=`uniform sampler2D tEquirect;
varying vec3 vWorldDirection;
#include <common>
void main() {
	vec3 direction = normalize( vWorldDirection );
	vec2 sampleUV = equirectUv( direction );
	gl_FragColor = texture2D( tEquirect, sampleUV );
	#include <tonemapping_fragment>
	#include <colorspace_fragment>
}`,CS=`uniform float scale;
attribute float lineDistance;
varying float vLineDistance;
#include <common>
#include <uv_pars_vertex>
#include <color_pars_vertex>
#include <fog_pars_vertex>
#include <morphtarget_pars_vertex>
#include <logdepthbuf_pars_vertex>
#include <clipping_planes_pars_vertex>
void main() {
	vLineDistance = scale * lineDistance;
	#include <uv_vertex>
	#include <color_vertex>
	#include <morphinstance_vertex>
	#include <morphcolor_vertex>
	#include <begin_vertex>
	#include <morphtarget_vertex>
	#include <project_vertex>
	#include <logdepthbuf_vertex>
	#include <clipping_planes_vertex>
	#include <fog_vertex>
}`,NS=`uniform vec3 diffuse;
uniform float opacity;
uniform float dashSize;
uniform float totalSize;
varying float vLineDistance;
#include <common>
#include <color_pars_fragment>
#include <uv_pars_fragment>
#include <map_pars_fragment>
#include <fog_pars_fragment>
#include <logdepthbuf_pars_fragment>
#include <clipping_planes_pars_fragment>
void main() {
	vec4 diffuseColor = vec4( diffuse, opacity );
	#include <clipping_planes_fragment>
	if ( mod( vLineDistance, totalSize ) > dashSize ) {
		discard;
	}
	vec3 outgoingLight = vec3( 0.0 );
	#include <logdepthbuf_fragment>
	#include <map_fragment>
	#include <color_fragment>
	outgoingLight = diffuseColor.rgb;
	#include <opaque_fragment>
	#include <tonemapping_fragment>
	#include <colorspace_fragment>
	#include <fog_fragment>
	#include <premultiplied_alpha_fragment>
}`,IS=`#include <common>
#include <batching_pars_vertex>
#include <uv_pars_vertex>
#include <envmap_pars_vertex>
#include <color_pars_vertex>
#include <fog_pars_vertex>
#include <morphtarget_pars_vertex>
#include <skinning_pars_vertex>
#include <logdepthbuf_pars_vertex>
#include <clipping_planes_pars_vertex>
void main() {
	#include <uv_vertex>
	#include <color_vertex>
	#include <morphinstance_vertex>
	#include <morphcolor_vertex>
	#include <batching_vertex>
	#if defined ( USE_ENVMAP ) || defined ( USE_SKINNING )
		#include <beginnormal_vertex>
		#include <morphnormal_vertex>
		#include <skinbase_vertex>
		#include <skinnormal_vertex>
		#include <defaultnormal_vertex>
	#endif
	#include <begin_vertex>
	#include <morphtarget_vertex>
	#include <skinning_vertex>
	#include <project_vertex>
	#include <logdepthbuf_vertex>
	#include <clipping_planes_vertex>
	#include <worldpos_vertex>
	#include <envmap_vertex>
	#include <fog_vertex>
}`,BS=`uniform vec3 diffuse;
uniform float opacity;
#ifndef FLAT_SHADED
	varying vec3 vNormal;
#endif
#include <common>
#include <dithering_pars_fragment>
#include <color_pars_fragment>
#include <uv_pars_fragment>
#include <map_pars_fragment>
#include <alphamap_pars_fragment>
#include <alphatest_pars_fragment>
#include <alphahash_pars_fragment>
#include <aomap_pars_fragment>
#include <lightmap_pars_fragment>
#include <envmap_common_pars_fragment>
#include <envmap_pars_fragment>
#include <fog_pars_fragment>
#include <specularmap_pars_fragment>
#include <logdepthbuf_pars_fragment>
#include <clipping_planes_pars_fragment>
void main() {
	vec4 diffuseColor = vec4( diffuse, opacity );
	#include <clipping_planes_fragment>
	#include <logdepthbuf_fragment>
	#include <map_fragment>
	#include <color_fragment>
	#include <alphamap_fragment>
	#include <alphatest_fragment>
	#include <alphahash_fragment>
	#include <specularmap_fragment>
	ReflectedLight reflectedLight = ReflectedLight( vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ) );
	#ifdef USE_LIGHTMAP
		vec4 lightMapTexel = texture2D( lightMap, vLightMapUv );
		reflectedLight.indirectDiffuse += lightMapTexel.rgb * lightMapIntensity * RECIPROCAL_PI;
	#else
		reflectedLight.indirectDiffuse += vec3( 1.0 );
	#endif
	#include <aomap_fragment>
	reflectedLight.indirectDiffuse *= diffuseColor.rgb;
	vec3 outgoingLight = reflectedLight.indirectDiffuse;
	#include <envmap_fragment>
	#include <opaque_fragment>
	#include <tonemapping_fragment>
	#include <colorspace_fragment>
	#include <fog_fragment>
	#include <premultiplied_alpha_fragment>
	#include <dithering_fragment>
}`,FS=`#define LAMBERT
varying vec3 vViewPosition;
#include <common>
#include <batching_pars_vertex>
#include <uv_pars_vertex>
#include <displacementmap_pars_vertex>
#include <envmap_pars_vertex>
#include <color_pars_vertex>
#include <fog_pars_vertex>
#include <normal_pars_vertex>
#include <morphtarget_pars_vertex>
#include <skinning_pars_vertex>
#include <shadowmap_pars_vertex>
#include <logdepthbuf_pars_vertex>
#include <clipping_planes_pars_vertex>
void main() {
	#include <uv_vertex>
	#include <color_vertex>
	#include <morphinstance_vertex>
	#include <morphcolor_vertex>
	#include <batching_vertex>
	#include <beginnormal_vertex>
	#include <morphnormal_vertex>
	#include <skinbase_vertex>
	#include <skinnormal_vertex>
	#include <defaultnormal_vertex>
	#include <normal_vertex>
	#include <begin_vertex>
	#include <morphtarget_vertex>
	#include <skinning_vertex>
	#include <displacementmap_vertex>
	#include <project_vertex>
	#include <logdepthbuf_vertex>
	#include <clipping_planes_vertex>
	vViewPosition = - mvPosition.xyz;
	#include <worldpos_vertex>
	#include <envmap_vertex>
	#include <shadowmap_vertex>
	#include <fog_vertex>
}`,LS=`#define LAMBERT
uniform vec3 diffuse;
uniform vec3 emissive;
uniform float opacity;
#include <common>
#include <dithering_pars_fragment>
#include <color_pars_fragment>
#include <uv_pars_fragment>
#include <map_pars_fragment>
#include <alphamap_pars_fragment>
#include <alphatest_pars_fragment>
#include <alphahash_pars_fragment>
#include <aomap_pars_fragment>
#include <lightmap_pars_fragment>
#include <emissivemap_pars_fragment>
#include <cube_uv_reflection_fragment>
#include <envmap_common_pars_fragment>
#include <envmap_pars_fragment>
#include <envmap_physical_pars_fragment>
#include <fog_pars_fragment>
#include <bsdfs>
#include <lights_pars_begin>
#include <normal_pars_fragment>
#include <lights_lambert_pars_fragment>
#include <shadowmap_pars_fragment>
#include <bumpmap_pars_fragment>
#include <normalmap_pars_fragment>
#include <specularmap_pars_fragment>
#include <logdepthbuf_pars_fragment>
#include <clipping_planes_pars_fragment>
void main() {
	vec4 diffuseColor = vec4( diffuse, opacity );
	#include <clipping_planes_fragment>
	ReflectedLight reflectedLight = ReflectedLight( vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ) );
	vec3 totalEmissiveRadiance = emissive;
	#include <logdepthbuf_fragment>
	#include <map_fragment>
	#include <color_fragment>
	#include <alphamap_fragment>
	#include <alphatest_fragment>
	#include <alphahash_fragment>
	#include <specularmap_fragment>
	#include <normal_fragment_begin>
	#include <normal_fragment_maps>
	#include <emissivemap_fragment>
	#include <lights_lambert_fragment>
	#include <lights_fragment_begin>
	#include <lights_fragment_maps>
	#include <lights_fragment_end>
	#include <aomap_fragment>
	vec3 outgoingLight = reflectedLight.directDiffuse + reflectedLight.indirectDiffuse + totalEmissiveRadiance;
	#include <envmap_fragment>
	#include <opaque_fragment>
	#include <tonemapping_fragment>
	#include <colorspace_fragment>
	#include <fog_fragment>
	#include <premultiplied_alpha_fragment>
	#include <dithering_fragment>
}`,US=`#define MATCAP
varying vec3 vViewPosition;
#include <common>
#include <batching_pars_vertex>
#include <uv_pars_vertex>
#include <color_pars_vertex>
#include <displacementmap_pars_vertex>
#include <fog_pars_vertex>
#include <normal_pars_vertex>
#include <morphtarget_pars_vertex>
#include <skinning_pars_vertex>
#include <logdepthbuf_pars_vertex>
#include <clipping_planes_pars_vertex>
void main() {
	#include <uv_vertex>
	#include <color_vertex>
	#include <morphinstance_vertex>
	#include <morphcolor_vertex>
	#include <batching_vertex>
	#include <beginnormal_vertex>
	#include <morphnormal_vertex>
	#include <skinbase_vertex>
	#include <skinnormal_vertex>
	#include <defaultnormal_vertex>
	#include <normal_vertex>
	#include <begin_vertex>
	#include <morphtarget_vertex>
	#include <skinning_vertex>
	#include <displacementmap_vertex>
	#include <project_vertex>
	#include <logdepthbuf_vertex>
	#include <clipping_planes_vertex>
	#include <fog_vertex>
	vViewPosition = - mvPosition.xyz;
}`,DS=`#define MATCAP
uniform vec3 diffuse;
uniform float opacity;
uniform sampler2D matcap;
varying vec3 vViewPosition;
#include <common>
#include <dithering_pars_fragment>
#include <color_pars_fragment>
#include <uv_pars_fragment>
#include <map_pars_fragment>
#include <alphamap_pars_fragment>
#include <alphatest_pars_fragment>
#include <alphahash_pars_fragment>
#include <fog_pars_fragment>
#include <normal_pars_fragment>
#include <bumpmap_pars_fragment>
#include <normalmap_pars_fragment>
#include <logdepthbuf_pars_fragment>
#include <clipping_planes_pars_fragment>
void main() {
	vec4 diffuseColor = vec4( diffuse, opacity );
	#include <clipping_planes_fragment>
	#include <logdepthbuf_fragment>
	#include <map_fragment>
	#include <color_fragment>
	#include <alphamap_fragment>
	#include <alphatest_fragment>
	#include <alphahash_fragment>
	#include <normal_fragment_begin>
	#include <normal_fragment_maps>
	vec3 viewDir = normalize( vViewPosition );
	vec3 x = normalize( vec3( viewDir.z, 0.0, - viewDir.x ) );
	vec3 y = cross( viewDir, x );
	vec2 uv = vec2( dot( x, normal ), dot( y, normal ) ) * 0.495 + 0.5;
	#ifdef USE_MATCAP
		vec4 matcapColor = texture2D( matcap, uv );
	#else
		vec4 matcapColor = vec4( vec3( mix( 0.2, 0.8, uv.y ) ), 1.0 );
	#endif
	vec3 outgoingLight = diffuseColor.rgb * matcapColor.rgb;
	#include <opaque_fragment>
	#include <tonemapping_fragment>
	#include <colorspace_fragment>
	#include <fog_fragment>
	#include <premultiplied_alpha_fragment>
	#include <dithering_fragment>
}`,PS=`#define NORMAL
#if defined( FLAT_SHADED ) || defined( USE_BUMPMAP ) || defined( USE_NORMALMAP_TANGENTSPACE )
	varying vec3 vViewPosition;
#endif
#include <common>
#include <batching_pars_vertex>
#include <uv_pars_vertex>
#include <displacementmap_pars_vertex>
#include <normal_pars_vertex>
#include <morphtarget_pars_vertex>
#include <skinning_pars_vertex>
#include <logdepthbuf_pars_vertex>
#include <clipping_planes_pars_vertex>
void main() {
	#include <uv_vertex>
	#include <batching_vertex>
	#include <beginnormal_vertex>
	#include <morphinstance_vertex>
	#include <morphnormal_vertex>
	#include <skinbase_vertex>
	#include <skinnormal_vertex>
	#include <defaultnormal_vertex>
	#include <normal_vertex>
	#include <begin_vertex>
	#include <morphtarget_vertex>
	#include <skinning_vertex>
	#include <displacementmap_vertex>
	#include <project_vertex>
	#include <logdepthbuf_vertex>
	#include <clipping_planes_vertex>
#if defined( FLAT_SHADED ) || defined( USE_BUMPMAP ) || defined( USE_NORMALMAP_TANGENTSPACE )
	vViewPosition = - mvPosition.xyz;
#endif
}`,GS=`#define NORMAL
uniform float opacity;
#if defined( FLAT_SHADED ) || defined( USE_BUMPMAP ) || defined( USE_NORMALMAP_TANGENTSPACE )
	varying vec3 vViewPosition;
#endif
#include <uv_pars_fragment>
#include <normal_pars_fragment>
#include <bumpmap_pars_fragment>
#include <normalmap_pars_fragment>
#include <logdepthbuf_pars_fragment>
#include <clipping_planes_pars_fragment>
void main() {
	vec4 diffuseColor = vec4( 0.0, 0.0, 0.0, opacity );
	#include <clipping_planes_fragment>
	#include <logdepthbuf_fragment>
	#include <normal_fragment_begin>
	#include <normal_fragment_maps>
	gl_FragColor = vec4( normalize( normal ) * 0.5 + 0.5, diffuseColor.a );
	#ifdef OPAQUE
		gl_FragColor.a = 1.0;
	#endif
}`,VS=`#define PHONG
varying vec3 vViewPosition;
#include <common>
#include <batching_pars_vertex>
#include <uv_pars_vertex>
#include <displacementmap_pars_vertex>
#include <envmap_pars_vertex>
#include <color_pars_vertex>
#include <fog_pars_vertex>
#include <normal_pars_vertex>
#include <morphtarget_pars_vertex>
#include <skinning_pars_vertex>
#include <shadowmap_pars_vertex>
#include <logdepthbuf_pars_vertex>
#include <clipping_planes_pars_vertex>
void main() {
	#include <uv_vertex>
	#include <color_vertex>
	#include <morphcolor_vertex>
	#include <batching_vertex>
	#include <beginnormal_vertex>
	#include <morphinstance_vertex>
	#include <morphnormal_vertex>
	#include <skinbase_vertex>
	#include <skinnormal_vertex>
	#include <defaultnormal_vertex>
	#include <normal_vertex>
	#include <begin_vertex>
	#include <morphtarget_vertex>
	#include <skinning_vertex>
	#include <displacementmap_vertex>
	#include <project_vertex>
	#include <logdepthbuf_vertex>
	#include <clipping_planes_vertex>
	vViewPosition = - mvPosition.xyz;
	#include <worldpos_vertex>
	#include <envmap_vertex>
	#include <shadowmap_vertex>
	#include <fog_vertex>
}`,OS=`#define PHONG
uniform vec3 diffuse;
uniform vec3 emissive;
uniform vec3 specular;
uniform float shininess;
uniform float opacity;
#include <common>
#include <dithering_pars_fragment>
#include <color_pars_fragment>
#include <uv_pars_fragment>
#include <map_pars_fragment>
#include <alphamap_pars_fragment>
#include <alphatest_pars_fragment>
#include <alphahash_pars_fragment>
#include <aomap_pars_fragment>
#include <lightmap_pars_fragment>
#include <emissivemap_pars_fragment>
#include <cube_uv_reflection_fragment>
#include <envmap_common_pars_fragment>
#include <envmap_pars_fragment>
#include <envmap_physical_pars_fragment>
#include <fog_pars_fragment>
#include <bsdfs>
#include <lights_pars_begin>
#include <normal_pars_fragment>
#include <lights_phong_pars_fragment>
#include <shadowmap_pars_fragment>
#include <bumpmap_pars_fragment>
#include <normalmap_pars_fragment>
#include <specularmap_pars_fragment>
#include <logdepthbuf_pars_fragment>
#include <clipping_planes_pars_fragment>
void main() {
	vec4 diffuseColor = vec4( diffuse, opacity );
	#include <clipping_planes_fragment>
	ReflectedLight reflectedLight = ReflectedLight( vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ) );
	vec3 totalEmissiveRadiance = emissive;
	#include <logdepthbuf_fragment>
	#include <map_fragment>
	#include <color_fragment>
	#include <alphamap_fragment>
	#include <alphatest_fragment>
	#include <alphahash_fragment>
	#include <specularmap_fragment>
	#include <normal_fragment_begin>
	#include <normal_fragment_maps>
	#include <emissivemap_fragment>
	#include <lights_phong_fragment>
	#include <lights_fragment_begin>
	#include <lights_fragment_maps>
	#include <lights_fragment_end>
	#include <aomap_fragment>
	vec3 outgoingLight = reflectedLight.directDiffuse + reflectedLight.indirectDiffuse + reflectedLight.directSpecular + reflectedLight.indirectSpecular + totalEmissiveRadiance;
	#include <envmap_fragment>
	#include <opaque_fragment>
	#include <tonemapping_fragment>
	#include <colorspace_fragment>
	#include <fog_fragment>
	#include <premultiplied_alpha_fragment>
	#include <dithering_fragment>
}`,kS=`#define STANDARD
varying vec3 vViewPosition;
#ifdef USE_TRANSMISSION
	varying vec3 vWorldPosition;
#endif
#include <common>
#include <batching_pars_vertex>
#include <uv_pars_vertex>
#include <displacementmap_pars_vertex>
#include <color_pars_vertex>
#include <fog_pars_vertex>
#include <normal_pars_vertex>
#include <morphtarget_pars_vertex>
#include <skinning_pars_vertex>
#include <shadowmap_pars_vertex>
#include <logdepthbuf_pars_vertex>
#include <clipping_planes_pars_vertex>
void main() {
	#include <uv_vertex>
	#include <color_vertex>
	#include <morphinstance_vertex>
	#include <morphcolor_vertex>
	#include <batching_vertex>
	#include <beginnormal_vertex>
	#include <morphnormal_vertex>
	#include <skinbase_vertex>
	#include <skinnormal_vertex>
	#include <defaultnormal_vertex>
	#include <normal_vertex>
	#include <begin_vertex>
	#include <morphtarget_vertex>
	#include <skinning_vertex>
	#include <displacementmap_vertex>
	#include <project_vertex>
	#include <logdepthbuf_vertex>
	#include <clipping_planes_vertex>
	vViewPosition = - mvPosition.xyz;
	#include <worldpos_vertex>
	#include <shadowmap_vertex>
	#include <fog_vertex>
#ifdef USE_TRANSMISSION
	vWorldPosition = worldPosition.xyz;
#endif
}`,zS=`#define STANDARD
#ifdef PHYSICAL
	#define IOR
	#define USE_SPECULAR
#endif
uniform vec3 diffuse;
uniform vec3 emissive;
uniform float roughness;
uniform float metalness;
uniform float opacity;
#ifdef IOR
	uniform float ior;
#endif
#ifdef USE_SPECULAR
	uniform float specularIntensity;
	uniform vec3 specularColor;
	#ifdef USE_SPECULAR_COLORMAP
		uniform sampler2D specularColorMap;
	#endif
	#ifdef USE_SPECULAR_INTENSITYMAP
		uniform sampler2D specularIntensityMap;
	#endif
#endif
#ifdef USE_CLEARCOAT
	uniform float clearcoat;
	uniform float clearcoatRoughness;
#endif
#ifdef USE_DISPERSION
	uniform float dispersion;
#endif
#ifdef USE_RETROREFLECTION
	uniform float retroreflectivity;
#endif
#ifdef USE_IRIDESCENCE
	uniform float iridescence;
	uniform float iridescenceIOR;
	uniform float iridescenceThicknessMinimum;
	uniform float iridescenceThicknessMaximum;
#endif
#ifdef USE_SHEEN
	uniform vec3 sheenColor;
	uniform float sheenRoughness;
	#ifdef USE_SHEEN_COLORMAP
		uniform sampler2D sheenColorMap;
	#endif
	#ifdef USE_SHEEN_ROUGHNESSMAP
		uniform sampler2D sheenRoughnessMap;
	#endif
#endif
#ifdef USE_ANISOTROPY
	uniform vec2 anisotropyVector;
	#ifdef USE_ANISOTROPYMAP
		uniform sampler2D anisotropyMap;
	#endif
#endif
varying vec3 vViewPosition;
#include <common>
#include <dithering_pars_fragment>
#include <color_pars_fragment>
#include <uv_pars_fragment>
#include <map_pars_fragment>
#include <alphamap_pars_fragment>
#include <alphatest_pars_fragment>
#include <alphahash_pars_fragment>
#include <aomap_pars_fragment>
#include <lightmap_pars_fragment>
#include <emissivemap_pars_fragment>
#include <iridescence_fragment>
#include <cube_uv_reflection_fragment>
#include <envmap_common_pars_fragment>
#include <envmap_physical_pars_fragment>
#include <fog_pars_fragment>
#include <lights_pars_begin>
#include <normal_pars_fragment>
#include <lights_physical_pars_fragment>
#include <transmission_pars_fragment>
#include <shadowmap_pars_fragment>
#include <bumpmap_pars_fragment>
#include <normalmap_pars_fragment>
#include <clearcoat_pars_fragment>
#include <iridescence_pars_fragment>
#include <roughnessmap_pars_fragment>
#include <metalnessmap_pars_fragment>
#include <logdepthbuf_pars_fragment>
#include <clipping_planes_pars_fragment>
void main() {
	vec4 diffuseColor = vec4( diffuse, opacity );
	#include <clipping_planes_fragment>
	ReflectedLight reflectedLight = ReflectedLight( vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ) );
	vec3 totalEmissiveRadiance = emissive;
	#include <logdepthbuf_fragment>
	#include <map_fragment>
	#include <color_fragment>
	#include <alphamap_fragment>
	#include <alphatest_fragment>
	#include <alphahash_fragment>
	#include <roughnessmap_fragment>
	#include <metalnessmap_fragment>
	#include <normal_fragment_begin>
	#include <normal_fragment_maps>
	#include <clearcoat_normal_fragment_begin>
	#include <clearcoat_normal_fragment_maps>
	#include <emissivemap_fragment>
	#include <lights_physical_fragment>
	#include <lights_fragment_begin>
	#include <lights_fragment_maps>
	#include <lights_fragment_end>
	#include <aomap_fragment>
	vec3 totalDiffuse = reflectedLight.directDiffuse + reflectedLight.indirectDiffuse;
	vec3 totalSpecular = reflectedLight.directSpecular + reflectedLight.indirectSpecular;
	#include <transmission_fragment>
	vec3 outgoingLight = totalDiffuse + totalSpecular + totalEmissiveRadiance;
	#ifdef USE_SHEEN
 
		outgoingLight = outgoingLight + sheenSpecularDirect + sheenSpecularIndirect;
 
 	#endif
	#ifdef USE_CLEARCOAT
		float dotNVcc = saturate( dot( geometryClearcoatNormal, geometryViewDir ) );
		vec3 Fcc = F_Schlick( material.clearcoatF0, material.clearcoatF90, dotNVcc );
		outgoingLight = outgoingLight * ( 1.0 - material.clearcoat * Fcc ) + ( clearcoatSpecularDirect + clearcoatSpecularIndirect ) * material.clearcoat;
	#endif
	#include <opaque_fragment>
	#include <tonemapping_fragment>
	#include <colorspace_fragment>
	#include <fog_fragment>
	#include <premultiplied_alpha_fragment>
	#include <dithering_fragment>
}`,WS=`#define TOON
varying vec3 vViewPosition;
#include <common>
#include <batching_pars_vertex>
#include <uv_pars_vertex>
#include <displacementmap_pars_vertex>
#include <color_pars_vertex>
#include <fog_pars_vertex>
#include <normal_pars_vertex>
#include <morphtarget_pars_vertex>
#include <skinning_pars_vertex>
#include <shadowmap_pars_vertex>
#include <logdepthbuf_pars_vertex>
#include <clipping_planes_pars_vertex>
void main() {
	#include <uv_vertex>
	#include <color_vertex>
	#include <morphinstance_vertex>
	#include <morphcolor_vertex>
	#include <batching_vertex>
	#include <beginnormal_vertex>
	#include <morphnormal_vertex>
	#include <skinbase_vertex>
	#include <skinnormal_vertex>
	#include <defaultnormal_vertex>
	#include <normal_vertex>
	#include <begin_vertex>
	#include <morphtarget_vertex>
	#include <skinning_vertex>
	#include <displacementmap_vertex>
	#include <project_vertex>
	#include <logdepthbuf_vertex>
	#include <clipping_planes_vertex>
	vViewPosition = - mvPosition.xyz;
	#include <worldpos_vertex>
	#include <shadowmap_vertex>
	#include <fog_vertex>
}`,HS=`#define TOON
uniform vec3 diffuse;
uniform vec3 emissive;
uniform float opacity;
#include <common>
#include <dithering_pars_fragment>
#include <color_pars_fragment>
#include <uv_pars_fragment>
#include <map_pars_fragment>
#include <alphamap_pars_fragment>
#include <alphatest_pars_fragment>
#include <alphahash_pars_fragment>
#include <aomap_pars_fragment>
#include <lightmap_pars_fragment>
#include <emissivemap_pars_fragment>
#include <gradientmap_pars_fragment>
#include <fog_pars_fragment>
#include <bsdfs>
#include <lights_pars_begin>
#include <normal_pars_fragment>
#include <lights_toon_pars_fragment>
#include <shadowmap_pars_fragment>
#include <bumpmap_pars_fragment>
#include <normalmap_pars_fragment>
#include <logdepthbuf_pars_fragment>
#include <clipping_planes_pars_fragment>
void main() {
	vec4 diffuseColor = vec4( diffuse, opacity );
	#include <clipping_planes_fragment>
	ReflectedLight reflectedLight = ReflectedLight( vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ), vec3( 0.0 ) );
	vec3 totalEmissiveRadiance = emissive;
	#include <logdepthbuf_fragment>
	#include <map_fragment>
	#include <color_fragment>
	#include <alphamap_fragment>
	#include <alphatest_fragment>
	#include <alphahash_fragment>
	#include <normal_fragment_begin>
	#include <normal_fragment_maps>
	#include <emissivemap_fragment>
	#include <lights_toon_fragment>
	#include <lights_fragment_begin>
	#include <lights_fragment_maps>
	#include <lights_fragment_end>
	#include <aomap_fragment>
	vec3 outgoingLight = reflectedLight.directDiffuse + reflectedLight.indirectDiffuse + totalEmissiveRadiance;
	#include <opaque_fragment>
	#include <tonemapping_fragment>
	#include <colorspace_fragment>
	#include <fog_fragment>
	#include <premultiplied_alpha_fragment>
	#include <dithering_fragment>
}`,YS=`uniform float size;
uniform float scale;
#include <common>
#include <color_pars_vertex>
#include <fog_pars_vertex>
#include <morphtarget_pars_vertex>
#include <logdepthbuf_pars_vertex>
#include <clipping_planes_pars_vertex>
#ifdef USE_POINTS_UV
	varying vec2 vUv;
	uniform mat3 uvTransform;
#endif
void main() {
	#ifdef USE_POINTS_UV
		vUv = ( uvTransform * vec3( uv, 1 ) ).xy;
	#endif
	#include <color_vertex>
	#include <morphinstance_vertex>
	#include <morphcolor_vertex>
	#include <begin_vertex>
	#include <morphtarget_vertex>
	#include <project_vertex>
	gl_PointSize = size;
	#ifdef USE_SIZEATTENUATION
		bool isPerspective = isPerspectiveMatrix( projectionMatrix );
		if ( isPerspective ) gl_PointSize *= ( scale / - mvPosition.z );
	#endif
	#include <logdepthbuf_vertex>
	#include <clipping_planes_vertex>
	#include <worldpos_vertex>
	#include <fog_vertex>
}`,ZS=`uniform vec3 diffuse;
uniform float opacity;
#include <common>
#include <color_pars_fragment>
#include <map_particle_pars_fragment>
#include <alphatest_pars_fragment>
#include <alphahash_pars_fragment>
#include <fog_pars_fragment>
#include <logdepthbuf_pars_fragment>
#include <clipping_planes_pars_fragment>
void main() {
	vec4 diffuseColor = vec4( diffuse, opacity );
	#include <clipping_planes_fragment>
	vec3 outgoingLight = vec3( 0.0 );
	#include <logdepthbuf_fragment>
	#include <map_particle_fragment>
	#include <color_fragment>
	#include <alphatest_fragment>
	#include <alphahash_fragment>
	outgoingLight = diffuseColor.rgb;
	#include <opaque_fragment>
	#include <tonemapping_fragment>
	#include <colorspace_fragment>
	#include <fog_fragment>
	#include <premultiplied_alpha_fragment>
}`,XS=`#include <common>
#include <batching_pars_vertex>
#include <fog_pars_vertex>
#include <morphtarget_pars_vertex>
#include <skinning_pars_vertex>
#include <logdepthbuf_pars_vertex>
#include <shadowmap_pars_vertex>
void main() {
	#include <batching_vertex>
	#include <beginnormal_vertex>
	#include <morphinstance_vertex>
	#include <morphnormal_vertex>
	#include <skinbase_vertex>
	#include <skinnormal_vertex>
	#include <defaultnormal_vertex>
	#include <begin_vertex>
	#include <morphtarget_vertex>
	#include <skinning_vertex>
	#include <project_vertex>
	#include <logdepthbuf_vertex>
	#include <worldpos_vertex>
	#include <shadowmap_vertex>
	#include <fog_vertex>
}`,KS=`uniform vec3 color;
uniform float opacity;
#include <common>
#include <fog_pars_fragment>
#include <bsdfs>
#include <lights_pars_begin>
#include <logdepthbuf_pars_fragment>
#include <shadowmap_pars_fragment>
#include <shadowmask_pars_fragment>
void main() {
	#include <logdepthbuf_fragment>
	gl_FragColor = vec4( color, opacity * ( 1.0 - getShadowMask() ) );
	#include <tonemapping_fragment>
	#include <colorspace_fragment>
	#include <fog_fragment>
	#include <premultiplied_alpha_fragment>
}`,$S=`uniform float rotation;
uniform vec2 center;
#include <common>
#include <uv_pars_vertex>
#include <fog_pars_vertex>
#include <logdepthbuf_pars_vertex>
#include <clipping_planes_pars_vertex>
void main() {
	#include <uv_vertex>
	vec4 mvPosition = modelViewMatrix[ 3 ];
	vec2 scale = vec2( length( modelMatrix[ 0 ].xyz ), length( modelMatrix[ 1 ].xyz ) );
	#ifndef USE_SIZEATTENUATION
		bool isPerspective = isPerspectiveMatrix( projectionMatrix );
		if ( isPerspective ) scale *= - mvPosition.z;
	#endif
	vec2 alignedPosition = ( position.xy - ( center - vec2( 0.5 ) ) ) * scale;
	vec2 rotatedPosition;
	rotatedPosition.x = cos( rotation ) * alignedPosition.x - sin( rotation ) * alignedPosition.y;
	rotatedPosition.y = sin( rotation ) * alignedPosition.x + cos( rotation ) * alignedPosition.y;
	mvPosition.xy += rotatedPosition;
	gl_Position = projectionMatrix * mvPosition;
	#include <logdepthbuf_vertex>
	#include <clipping_planes_vertex>
	#include <fog_vertex>
}`,JS=`uniform vec3 diffuse;
uniform float opacity;
#include <common>
#include <uv_pars_fragment>
#include <map_pars_fragment>
#include <alphamap_pars_fragment>
#include <alphatest_pars_fragment>
#include <alphahash_pars_fragment>
#include <fog_pars_fragment>
#include <logdepthbuf_pars_fragment>
#include <clipping_planes_pars_fragment>
void main() {
	vec4 diffuseColor = vec4( diffuse, opacity );
	#include <clipping_planes_fragment>
	vec3 outgoingLight = vec3( 0.0 );
	#include <logdepthbuf_fragment>
	#include <map_fragment>
	#include <alphamap_fragment>
	#include <alphatest_fragment>
	#include <alphahash_fragment>
	outgoingLight = diffuseColor.rgb;
	#include <opaque_fragment>
	#include <tonemapping_fragment>
	#include <colorspace_fragment>
	#include <fog_fragment>
}`,Xe={alphahash_fragment:x_,alphahash_pars_fragment:y_,alphamap_fragment:__,alphamap_pars_fragment:b_,alphatest_fragment:S_,alphatest_pars_fragment:v_,aomap_fragment:A_,aomap_pars_fragment:T_,batching_pars_vertex:E_,batching_vertex:M_,begin_vertex:R_,beginnormal_vertex:w_,bsdfs:C_,iridescence_fragment:N_,bumpmap_pars_fragment:I_,clipping_planes_fragment:B_,clipping_planes_pars_fragment:F_,clipping_planes_pars_vertex:L_,clipping_planes_vertex:U_,color_fragment:D_,color_pars_fragment:P_,color_pars_vertex:G_,color_vertex:V_,common:O_,cube_uv_reflection_fragment:k_,defaultnormal_vertex:z_,displacementmap_pars_vertex:W_,displacementmap_vertex:H_,emissivemap_fragment:Y_,emissivemap_pars_fragment:Z_,colorspace_fragment:X_,colorspace_pars_fragment:K_,envmap_fragment:$_,envmap_common_pars_fragment:J_,envmap_pars_fragment:q_,envmap_pars_vertex:Q_,envmap_physical_pars_fragment:cb,envmap_vertex:j_,fog_vertex:eb,fog_pars_vertex:tb,fog_fragment:nb,fog_pars_fragment:ib,gradientmap_pars_fragment:sb,lightmap_pars_fragment:rb,lights_lambert_fragment:ob,lights_lambert_pars_fragment:ab,lights_pars_begin:lb,lights_toon_fragment:ub,lights_toon_pars_fragment:hb,lights_phong_fragment:db,lights_phong_pars_fragment:fb,lights_physical_fragment:pb,lights_physical_pars_fragment:mb,lights_fragment_begin:gb,lights_fragment_maps:xb,lights_fragment_end:yb,lightprobes_pars_fragment:_b,logdepthbuf_fragment:bb,logdepthbuf_pars_fragment:Sb,logdepthbuf_pars_vertex:vb,logdepthbuf_vertex:Ab,map_fragment:Tb,map_pars_fragment:Eb,map_particle_fragment:Mb,map_particle_pars_fragment:Rb,metalnessmap_fragment:wb,metalnessmap_pars_fragment:Cb,morphinstance_vertex:Nb,morphcolor_vertex:Ib,morphnormal_vertex:Bb,morphtarget_pars_vertex:Fb,morphtarget_vertex:Lb,normal_fragment_begin:Ub,normal_fragment_maps:Db,normal_pars_fragment:Pb,normal_pars_vertex:Gb,normal_vertex:Vb,normalmap_pars_fragment:Ob,clearcoat_normal_fragment_begin:kb,clearcoat_normal_fragment_maps:zb,clearcoat_pars_fragment:Wb,iridescence_pars_fragment:Hb,opaque_fragment:Yb,packing:Zb,premultiplied_alpha_fragment:Xb,project_vertex:Kb,dithering_fragment:$b,dithering_pars_fragment:Jb,roughnessmap_fragment:qb,roughnessmap_pars_fragment:Qb,shadowmap_pars_fragment:jb,shadowmap_pars_vertex:eS,shadowmap_vertex:tS,shadowmask_pars_fragment:nS,skinbase_vertex:iS,skinning_pars_vertex:sS,skinning_vertex:rS,skinnormal_vertex:oS,specularmap_fragment:aS,specularmap_pars_fragment:lS,tonemapping_fragment:cS,tonemapping_pars_fragment:uS,transmission_fragment:hS,transmission_pars_fragment:dS,uv_pars_fragment:fS,uv_pars_vertex:pS,uv_vertex:mS,worldpos_vertex:gS,background_vert:xS,background_frag:yS,backgroundCube_vert:_S,backgroundCube_frag:bS,cube_vert:SS,cube_frag:vS,depth_vert:AS,depth_frag:TS,distance_vert:ES,distance_frag:MS,equirect_vert:RS,equirect_frag:wS,linedashed_vert:CS,linedashed_frag:NS,meshbasic_vert:IS,meshbasic_frag:BS,meshlambert_vert:FS,meshlambert_frag:LS,meshmatcap_vert:US,meshmatcap_frag:DS,meshnormal_vert:PS,meshnormal_frag:GS,meshphong_vert:VS,meshphong_frag:OS,meshphysical_vert:kS,meshphysical_frag:zS,meshtoon_vert:WS,meshtoon_frag:HS,points_vert:YS,points_frag:ZS,shadow_vert:XS,shadow_frag:KS,sprite_vert:$S,sprite_frag:JS},me={common:{diffuse:{value:new He(16777215)},opacity:{value:1},map:{value:null},mapTransform:{value:new Ge},alphaMap:{value:null},alphaMapTransform:{value:new Ge},alphaTest:{value:0}},specularmap:{specularMap:{value:null},specularMapTransform:{value:new Ge}},envmap:{envMap:{value:null},envMapRotation:{value:new Ge},reflectivity:{value:1},ior:{value:1.5},refractionRatio:{value:.98},dfgLUT:{value:null}},aomap:{aoMap:{value:null},aoMapIntensity:{value:1},aoMapTransform:{value:new Ge}},lightmap:{lightMap:{value:null},lightMapIntensity:{value:1},lightMapTransform:{value:new Ge}},bumpmap:{bumpMap:{value:null},bumpMapTransform:{value:new Ge},bumpScale:{value:1}},normalmap:{normalMap:{value:null},normalMapTransform:{value:new Ge},normalScale:{value:new ze(1,1)}},displacementmap:{displacementMap:{value:null},displacementMapTransform:{value:new Ge},displacementScale:{value:1},displacementBias:{value:0}},emissivemap:{emissiveMap:{value:null},emissiveMapTransform:{value:new Ge}},metalnessmap:{metalnessMap:{value:null},metalnessMapTransform:{value:new Ge}},roughnessmap:{roughnessMap:{value:null},roughnessMapTransform:{value:new Ge}},gradientmap:{gradientMap:{value:null}},fog:{fogDensity:{value:25e-5},fogNear:{value:1},fogFar:{value:2e3},fogColor:{value:new He(16777215)}},lights:{ambientLightColor:{value:[]},lightProbe:{value:[]},sunLights:{value:[],properties:{direction:{},color:{}}},sunLightShadows:{value:[],properties:{shadowIntensity:1,shadowBias:{},shadowNormalBias:{},shadowRadius:{},shadowMapSize:{}}},sunShadowMatrix:{value:[]},sunShadowCascade:{value:[]},directionalLights:{value:[],properties:{direction:{},color:{}}},directionalLightShadows:{value:[],properties:{shadowIntensity:1,shadowBias:{},shadowNormalBias:{},shadowRadius:{},shadowMapSize:{}}},directionalShadowMatrix:{value:[]},spotLights:{value:[],properties:{color:{},position:{},direction:{},distance:{},coneCos:{},penumbraCos:{},decay:{}}},spotLightShadows:{value:[],properties:{shadowIntensity:1,shadowBias:{},shadowNormalBias:{},shadowRadius:{},shadowMapSize:{}}},spotLightMap:{value:[]},spotLightMatrix:{value:[]},pointLights:{value:[],properties:{color:{},position:{},decay:{},distance:{}}},pointLightShadows:{value:[],properties:{shadowIntensity:1,shadowBias:{},shadowNormalBias:{},shadowRadius:{},shadowMapSize:{},shadowCameraNear:{},shadowCameraFar:{}}},pointShadowMatrix:{value:[]},hemisphereLights:{value:[],properties:{direction:{},skyColor:{},groundColor:{}}},rectAreaLights:{value:[],properties:{color:{},position:{},width:{},height:{}}},ltc_1:{value:null},ltc_2:{value:null},probesSH:{value:null},probesMin:{value:new L},probesMax:{value:new L},probesResolution:{value:new L}},points:{diffuse:{value:new He(16777215)},opacity:{value:1},size:{value:1},scale:{value:1},map:{value:null},alphaMap:{value:null},alphaMapTransform:{value:new Ge},alphaTest:{value:0},uvTransform:{value:new Ge}},sprite:{diffuse:{value:new He(16777215)},opacity:{value:1},center:{value:new ze(.5,.5)},rotation:{value:0},map:{value:null},mapTransform:{value:new Ge},alphaMap:{value:null},alphaMapTransform:{value:new Ge},alphaTest:{value:0}}},mi={basic:{uniforms:rn([me.common,me.specularmap,me.envmap,me.aomap,me.lightmap,me.fog]),vertexShader:Xe.meshbasic_vert,fragmentShader:Xe.meshbasic_frag},lambert:{uniforms:rn([me.common,me.specularmap,me.envmap,me.aomap,me.lightmap,me.emissivemap,me.bumpmap,me.normalmap,me.displacementmap,me.fog,me.lights,{emissive:{value:new He(0)},envMapIntensity:{value:1}}]),vertexShader:Xe.meshlambert_vert,fragmentShader:Xe.meshlambert_frag},phong:{uniforms:rn([me.common,me.specularmap,me.envmap,me.aomap,me.lightmap,me.emissivemap,me.bumpmap,me.normalmap,me.displacementmap,me.fog,me.lights,{emissive:{value:new He(0)},specular:{value:new He(1118481)},shininess:{value:30},envMapIntensity:{value:1}}]),vertexShader:Xe.meshphong_vert,fragmentShader:Xe.meshphong_frag},standard:{uniforms:rn([me.common,me.envmap,me.aomap,me.lightmap,me.emissivemap,me.bumpmap,me.normalmap,me.displacementmap,me.roughnessmap,me.metalnessmap,me.fog,me.lights,{emissive:{value:new He(0)},roughness:{value:1},metalness:{value:0},envMapIntensity:{value:1}}]),vertexShader:Xe.meshphysical_vert,fragmentShader:Xe.meshphysical_frag},toon:{uniforms:rn([me.common,me.aomap,me.lightmap,me.emissivemap,me.bumpmap,me.normalmap,me.displacementmap,me.gradientmap,me.fog,me.lights,{emissive:{value:new He(0)}}]),vertexShader:Xe.meshtoon_vert,fragmentShader:Xe.meshtoon_frag},matcap:{uniforms:rn([me.common,me.bumpmap,me.normalmap,me.displacementmap,me.fog,{matcap:{value:null}}]),vertexShader:Xe.meshmatcap_vert,fragmentShader:Xe.meshmatcap_frag},points:{uniforms:rn([me.points,me.fog]),vertexShader:Xe.points_vert,fragmentShader:Xe.points_frag},dashed:{uniforms:rn([me.common,me.fog,{scale:{value:1},dashSize:{value:1},totalSize:{value:2}}]),vertexShader:Xe.linedashed_vert,fragmentShader:Xe.linedashed_frag},depth:{uniforms:rn([me.common,me.displacementmap]),vertexShader:Xe.depth_vert,fragmentShader:Xe.depth_frag},normal:{uniforms:rn([me.common,me.bumpmap,me.normalmap,me.displacementmap,{opacity:{value:1}}]),vertexShader:Xe.meshnormal_vert,fragmentShader:Xe.meshnormal_frag},sprite:{uniforms:rn([me.sprite,me.fog]),vertexShader:Xe.sprite_vert,fragmentShader:Xe.sprite_frag},background:{uniforms:{uvTransform:{value:new Ge},t2D:{value:null},backgroundIntensity:{value:1}},vertexShader:Xe.background_vert,fragmentShader:Xe.background_frag},backgroundCube:{uniforms:{envMap:{value:null},backgroundBlurriness:{value:0},backgroundIntensity:{value:1},backgroundRotation:{value:new Ge}},vertexShader:Xe.backgroundCube_vert,fragmentShader:Xe.backgroundCube_frag},cube:{uniforms:{tCube:{value:null},tFlip:{value:-1},opacity:{value:1}},vertexShader:Xe.cube_vert,fragmentShader:Xe.cube_frag},equirect:{uniforms:{tEquirect:{value:null}},vertexShader:Xe.equirect_vert,fragmentShader:Xe.equirect_frag},distance:{uniforms:rn([me.common,me.displacementmap,{referencePosition:{value:new L},nearDistance:{value:1},farDistance:{value:1e3}}]),vertexShader:Xe.distance_vert,fragmentShader:Xe.distance_frag},shadow:{uniforms:rn([me.lights,me.fog,{color:{value:new He(0)},opacity:{value:1}}]),vertexShader:Xe.shadow_vert,fragmentShader:Xe.shadow_frag}};mi.physical={uniforms:rn([mi.standard.uniforms,{clearcoat:{value:0},clearcoatMap:{value:null},clearcoatMapTransform:{value:new Ge},clearcoatNormalMap:{value:null},clearcoatNormalMapTransform:{value:new Ge},clearcoatNormalScale:{value:new ze(1,1)},clearcoatRoughness:{value:0},clearcoatRoughnessMap:{value:null},clearcoatRoughnessMapTransform:{value:new Ge},dispersion:{value:0},retroreflectivity:{value:0},iridescence:{value:0},iridescenceMap:{value:null},iridescenceMapTransform:{value:new Ge},iridescenceIOR:{value:1.3},iridescenceThicknessMinimum:{value:100},iridescenceThicknessMaximum:{value:400},iridescenceThicknessMap:{value:null},iridescenceThicknessMapTransform:{value:new Ge},sheen:{value:0},sheenColor:{value:new He(0)},sheenColorMap:{value:null},sheenColorMapTransform:{value:new Ge},sheenRoughness:{value:1},sheenRoughnessMap:{value:null},sheenRoughnessMapTransform:{value:new Ge},transmission:{value:0},transmissionMap:{value:null},transmissionMapTransform:{value:new Ge},transmissionSamplerSize:{value:new ze},transmissionSamplerMap:{value:null},thickness:{value:0},thicknessMap:{value:null},thicknessMapTransform:{value:new Ge},attenuationDistance:{value:0},attenuationColor:{value:new He(0)},specularColor:{value:new He(1,1,1)},specularColorMap:{value:null},specularColorMapTransform:{value:new Ge},specularIntensity:{value:1},specularIntensityMap:{value:null},specularIntensityMapTransform:{value:new Ge},anisotropyVector:{value:new ze},anisotropyMap:{value:null},anisotropyMapTransform:{value:new Ge}}]),vertexShader:Xe.meshphysical_vert,fragmentShader:Xe.meshphysical_frag};var fc={r:0,b:0,g:0},qS=new ct,wm=new Ge;wm.set(-1,0,0,0,1,0,0,0,1);function QS(n,e,t,i,s,r){let o=new He(0),a=s===!0?0:1,l,c,u=null,d=0,h=null;function f(E){let w=E.isScene===!0?E.background:null;if(w&&w.isTexture){let S=E.backgroundBlurriness>0;w=e.get(w,S)}return w}function g(E){let w=!1,S=f(E);S===null?m(o,a):S&&S.isColor&&(m(S,1),w=!0);let v=n.xr.getEnvironmentBlendMode();v==="additive"?t.buffers.color.setClear(0,0,0,1,r):v==="alpha-blend"&&t.buffers.color.setClear(0,0,0,0,r),(n.autoClear||w)&&(t.buffers.depth.setTest(!0),t.buffers.depth.setMask(!0),t.buffers.color.setMask(!0),n.clear(n.autoClearColor,n.autoClearDepth,n.autoClearStencil))}function y(E,w){let S=f(w);S&&(S.isCubeTexture||S.mapping===Er)?(c===void 0&&(c=new fn(new ys(1,1,1),new Cn({name:"BackgroundCubeMaterial",uniforms:Cs(mi.backgroundCube.uniforms),vertexShader:mi.backgroundCube.vertexShader,fragmentShader:mi.backgroundCube.fragmentShader,side:Dt,depthTest:!1,depthWrite:!1,fog:!1,allowOverride:!1})),c.geometry.deleteAttribute("normal"),c.geometry.deleteAttribute("uv"),c.onBeforeRender=function(v,A,R){this.matrixWorld.copyPosition(R.matrixWorld)},Object.defineProperty(c.material,"envMap",{get:function(){return this.uniforms.envMap.value}}),i.update(c)),c.material.uniforms.envMap.value=S,c.material.uniforms.backgroundBlurriness.value=w.backgroundBlurriness,c.material.uniforms.backgroundIntensity.value=w.backgroundIntensity,c.material.uniforms.backgroundRotation.value.setFromMatrix4(qS.makeRotationFromEuler(w.backgroundRotation)).transpose(),S.isCubeTexture&&S.isRenderTargetTexture===!1&&c.material.uniforms.backgroundRotation.value.premultiply(wm),c.material.toneMapped=Ve.getTransfer(S.colorSpace)!==nt,(u!==S||d!==S.version||h!==n.toneMapping)&&(c.material.needsUpdate=!0,u=S,d=S.version,h=n.toneMapping),c.layers.enableAll(),E.unshift(c,c.geometry,c.material,0,0,null)):S&&S.isTexture&&(l===void 0&&(l=new fn(new br(2,2),new Cn({name:"BackgroundMaterial",uniforms:Cs(mi.background.uniforms),vertexShader:mi.background.vertexShader,fragmentShader:mi.background.fragmentShader,side:Jn,depthTest:!1,depthWrite:!1,fog:!1,allowOverride:!1})),l.geometry.deleteAttribute("normal"),Object.defineProperty(l.material,"map",{get:function(){return this.uniforms.t2D.value}}),i.update(l)),l.material.uniforms.t2D.value=S,l.material.uniforms.backgroundIntensity.value=w.backgroundIntensity,l.material.toneMapped=Ve.getTransfer(S.colorSpace)!==nt,S.matrixAutoUpdate===!0&&S.updateMatrix(),l.material.uniforms.uvTransform.value.copy(S.matrix),(u!==S||d!==S.version||h!==n.toneMapping)&&(l.material.needsUpdate=!0,u=S,d=S.version,h=n.toneMapping),l.layers.enableAll(),E.unshift(l,l.geometry,l.material,0,0,null))}function m(E,w){E.getRGB(fc,Uh(n)),t.buffers.color.setClear(fc.r,fc.g,fc.b,w,r)}function p(){c!==void 0&&(c.geometry.dispose(),c.material.dispose(),c=void 0),l!==void 0&&(l.geometry.dispose(),l.material.dispose(),l=void 0)}return{getClearColor:function(){return o},setClearColor:function(E,w=1){o.set(E),a=w,m(o,a)},getClearAlpha:function(){return a},setClearAlpha:function(E){a=E,m(o,a)},render:g,addToRenderList:y,dispose:p}}function jS(n,e){let t=n.getParameter(n.MAX_VERTEX_ATTRIBS),i={},s=h(null),r=s,o=!1;function a(D,k,H,F,O){let q=!1,X=d(D,F,H,k);r!==X&&(r=X,c(r.object)),q=f(D,F,H,O),q&&g(D,F,H,O),O!==null&&e.update(O,n.ELEMENT_ARRAY_BUFFER),(q||o)&&(o=!1,S(D,k,H,F),O!==null&&n.bindBuffer(n.ELEMENT_ARRAY_BUFFER,e.get(O).buffer))}function l(){return n.createVertexArray()}function c(D){return n.bindVertexArray(D)}function u(D){return n.deleteVertexArray(D)}function d(D,k,H,F){let O=F.wireframe===!0,q=i[k.id];q===void 0&&(q={},i[k.id]=q);let X=D.isInstancedMesh===!0?D.id:0,re=q[X];re===void 0&&(re={},q[X]=re);let K=re[H.id];K===void 0&&(K={},re[H.id]=K);let ne=K[O];return ne===void 0&&(ne=h(l()),K[O]=ne),ne}function h(D){let k=[],H=[],F=[];for(let O=0;O<t;O++)k[O]=0,H[O]=0,F[O]=0;return{geometry:null,program:null,wireframe:!1,newAttributes:k,enabledAttributes:H,attributeDivisors:F,object:D,attributes:{},index:null}}function f(D,k,H,F){let O=r.attributes,q=k.attributes,X=0,re=H.getAttributes();for(let K in re)if(re[K].location>=0){let se=O[K],Ue=q[K];if(Ue===void 0&&(K==="instanceMatrix"&&D.instanceMatrix&&(Ue=D.instanceMatrix),K==="instanceColor"&&D.instanceColor&&(Ue=D.instanceColor)),se===void 0||se.attribute!==Ue||Ue&&se.data!==Ue.data)return!0;X++}return r.attributesNum!==X||r.index!==F}function g(D,k,H,F){let O={},q=k.attributes,X=0,re=H.getAttributes();for(let K in re)if(re[K].location>=0){let se=q[K];se===void 0&&(K==="instanceMatrix"&&D.instanceMatrix&&(se=D.instanceMatrix),K==="instanceColor"&&D.instanceColor&&(se=D.instanceColor));let Ue={};Ue.attribute=se,se&&se.data&&(Ue.data=se.data),O[K]=Ue,X++}r.attributes=O,r.attributesNum=X,r.index=F}function y(){let D=r.newAttributes;for(let k=0,H=D.length;k<H;k++)D[k]=0}function m(D){p(D,0)}function p(D,k){let H=r.newAttributes,F=r.enabledAttributes,O=r.attributeDivisors;H[D]=1,F[D]===0&&(n.enableVertexAttribArray(D),F[D]=1),O[D]!==k&&(n.vertexAttribDivisor(D,k),O[D]=k)}function E(){let D=r.newAttributes,k=r.enabledAttributes;for(let H=0,F=k.length;H<F;H++)k[H]!==D[H]&&(n.disableVertexAttribArray(H),k[H]=0)}function w(D,k,H,F,O,q,X){X===!0?n.vertexAttribIPointer(D,k,H,O,q):n.vertexAttribPointer(D,k,H,F,O,q)}function S(D,k,H,F){y();let O=F.attributes,q=H.getAttributes(),X=k.defaultAttributeValues;for(let re in q){let K=q[re];if(K.location>=0){let ne=O[re];if(ne===void 0&&(re==="instanceMatrix"&&D.instanceMatrix&&(ne=D.instanceMatrix),re==="instanceColor"&&D.instanceColor&&(ne=D.instanceColor)),ne!==void 0){let se=ne.normalized,Ue=ne.itemSize,Fe=e.get(ne);if(Fe===void 0)continue;let yt=Fe.buffer,je=Fe.type,rt=Fe.bytesPerElement,$=je===n.INT||je===n.UNSIGNED_INT||ne.gpuType===Ts;if(ne.isInterleavedBufferAttribute){let te=ne.data,Te=te.stride,Oe=ne.offset;if(te.isInstancedInterleavedBuffer){for(let ve=0;ve<K.locationSize;ve++)p(K.location+ve,te.meshPerAttribute);D.isInstancedMesh!==!0&&F._maxInstanceCount===void 0&&(F._maxInstanceCount=te.meshPerAttribute*te.count)}else for(let ve=0;ve<K.locationSize;ve++)m(K.location+ve);n.bindBuffer(n.ARRAY_BUFFER,yt);for(let ve=0;ve<K.locationSize;ve++)w(K.location+ve,Ue/K.locationSize,je,se,Te*rt,(Oe+Ue/K.locationSize*ve)*rt,$)}else{if(ne.isInstancedBufferAttribute){for(let te=0;te<K.locationSize;te++)p(K.location+te,ne.meshPerAttribute);D.isInstancedMesh!==!0&&F._maxInstanceCount===void 0&&(F._maxInstanceCount=ne.meshPerAttribute*ne.count)}else for(let te=0;te<K.locationSize;te++)m(K.location+te);n.bindBuffer(n.ARRAY_BUFFER,yt);for(let te=0;te<K.locationSize;te++)w(K.location+te,Ue/K.locationSize,je,se,Ue*rt,Ue/K.locationSize*te*rt,$)}}else if(X!==void 0){let se=X[re];if(se!==void 0)switch(se.length){case 2:n.vertexAttrib2fv(K.location,se);break;case 3:n.vertexAttrib3fv(K.location,se);break;case 4:n.vertexAttrib4fv(K.location,se);break;default:n.vertexAttrib1fv(K.location,se)}}}}E()}function v(){M();for(let D in i){let k=i[D];for(let H in k){let F=k[H];for(let O in F){let q=F[O];for(let X in q)u(q[X].object),delete q[X];delete F[O]}}delete i[D]}}function A(D){if(i[D.id]===void 0)return;let k=i[D.id];for(let H in k){let F=k[H];for(let O in F){let q=F[O];for(let X in q)u(q[X].object),delete q[X];delete F[O]}}delete i[D.id]}function R(D){for(let k in i){let H=i[k];for(let F in H){let O=H[F];if(O[D.id]===void 0)continue;let q=O[D.id];for(let X in q)u(q[X].object),delete q[X];delete O[D.id]}}}function b(D){for(let k in i){let H=i[k],F=D.isInstancedMesh===!0?D.id:0,O=H[F];if(O!==void 0){for(let q in O){let X=O[q];for(let re in X)u(X[re].object),delete X[re];delete O[q]}delete H[F],Object.keys(H).length===0&&delete i[k]}}}function M(){B(),o=!0,r!==s&&(r=s,c(r.object))}function B(){s.geometry=null,s.program=null,s.wireframe=!1}return{setup:a,reset:M,resetDefaultState:B,dispose:v,releaseStatesOfGeometry:A,releaseStatesOfObject:b,releaseStatesOfProgram:R,initAttributes:y,enableAttribute:m,disableUnusedAttributes:E}}function ev(n,e,t){let i;function s(l){i=l}function r(l,c){n.drawArrays(i,l,c),t.update(c,i,1)}function o(l,c,u){u!==0&&(n.drawArraysInstanced(i,l,c,u),t.update(c,i,u))}function a(l,c,u){if(u===0)return;e.get("WEBGL_multi_draw").multiDrawArraysWEBGL(i,l,0,c,0,u);let h=0;for(let f=0;f<u;f++)h+=c[f];t.update(h,i,1)}this.setMode=s,this.render=r,this.renderInstances=o,this.renderMultiDraw=a}function tv(n,e,t,i){let s;function r(){if(s!==void 0)return s;if(e.has("EXT_texture_filter_anisotropic")===!0){let R=e.get("EXT_texture_filter_anisotropic");s=n.getParameter(R.MAX_TEXTURE_MAX_ANISOTROPY_EXT)}else s=0;return s}function o(R){return!(R!==gn&&i.convert(R)!==n.getParameter(n.IMPLEMENTATION_COLOR_READ_FORMAT))}function a(R){let b=R===Sn&&(e.has("EXT_color_buffer_half_float")||e.has("EXT_color_buffer_float"));return!(R!==mn&&R!==Dn&&!b&&i.convert(R)!==n.getParameter(n.IMPLEMENTATION_COLOR_READ_TYPE))}function l(R){if(R==="highp"){if(n.getShaderPrecisionFormat(n.VERTEX_SHADER,n.HIGH_FLOAT).precision>0&&n.getShaderPrecisionFormat(n.FRAGMENT_SHADER,n.HIGH_FLOAT).precision>0)return"highp";R="mediump"}return R==="mediump"&&n.getShaderPrecisionFormat(n.VERTEX_SHADER,n.MEDIUM_FLOAT).precision>0&&n.getShaderPrecisionFormat(n.FRAGMENT_SHADER,n.MEDIUM_FLOAT).precision>0?"mediump":"lowp"}let c=t.precision!==void 0?t.precision:"highp",u=l(c);u!==c&&(pe("WebGLRenderer:",c,"not supported, using",u,"instead."),c=u);let d=t.logarithmicDepthBuffer===!0,h=t.reversedDepthBuffer===!0&&e.has("EXT_clip_control");t.reversedDepthBuffer===!0&&h===!1&&pe("WebGLRenderer: Unable to use reversed depth buffer due to missing EXT_clip_control extension. Fallback to default depth buffer.");let f=n.getParameter(n.MAX_TEXTURE_IMAGE_UNITS),g=n.getParameter(n.MAX_VERTEX_TEXTURE_IMAGE_UNITS),y=n.getParameter(n.MAX_TEXTURE_SIZE),m=n.getParameter(n.MAX_CUBE_MAP_TEXTURE_SIZE),p=n.getParameter(n.MAX_VERTEX_ATTRIBS),E=n.getParameter(n.MAX_VERTEX_UNIFORM_VECTORS),w=n.getParameter(n.MAX_VARYING_VECTORS),S=n.getParameter(n.MAX_FRAGMENT_UNIFORM_VECTORS),v=n.getParameter(n.MAX_SAMPLES),A=n.getParameter(n.SAMPLES);return{isWebGL2:!0,getMaxAnisotropy:r,getMaxPrecision:l,textureFormatReadable:o,textureTypeReadable:a,precision:c,logarithmicDepthBuffer:d,reversedDepthBuffer:h,maxTextures:f,maxVertexTextures:g,maxTextureSize:y,maxCubemapSize:m,maxAttributes:p,maxVertexUniforms:E,maxVaryings:w,maxFragmentUniforms:S,maxSamples:v,samples:A}}function nv(n){let e=this,t=null,i=0,s=!1,r=!1,o=new Rn,a=new Ge,l={value:null,needsUpdate:!1};this.uniform=l,this.numPlanes=0,this.numIntersection=0,this.init=function(d,h){let f=d.length!==0||h||i!==0||s;return s=h,i=d.length,f},this.beginShadows=function(){r=!0,u(null)},this.endShadows=function(){r=!1},this.setGlobalState=function(d,h){t=u(d,h,0)},this.setState=function(d,h,f){let g=d.clippingPlanes,y=d.clipIntersection,m=d.clipShadows,p=n.get(d);if(!s||g===null||g.length===0||r&&!m)r?u(null):c();else{let E=r?0:i,w=E*4,S=p.clippingState||null;l.value=S,S=u(g,h,w,f);for(let v=0;v!==w;++v)S[v]=t[v];p.clippingState=S,this.numIntersection=y?this.numPlanes:0,this.numPlanes+=E}};function c(){l.value!==t&&(l.value=t,l.needsUpdate=i>0),e.numPlanes=i,e.numIntersection=0}function u(d,h,f,g){let y=d!==null?d.length:0,m=null;if(y!==0){if(m=l.value,g!==!0||m===null){let p=f+y*4,E=h.matrixWorldInverse;a.getNormalMatrix(E),(m===null||m.length<p)&&(m=new Float32Array(p));for(let w=0,S=f;w!==y;++w,S+=4)o.copy(d[w]).applyMatrix4(E,a),o.normal.toArray(m,S),m[S+3]=o.constant}l.value=m,l.needsUpdate=!0}return e.numPlanes=y,e.numIntersection=0,m}}var Dr=4,iv=6,sv=20,rv=256,ga=new _s,rm=new He,Gh=null,Vh=0,Oh=0,kh=!1,ov=new L,Ns=new L,mc=class{constructor(e){this._renderer=e,this._pingPongRenderTarget=null,this._lodMax=0,this._cubeSize=0,this._sizeLods=[],this._lodMeshes=[],this._backgroundBox=null,this._cubemapMaterial=null,this._equirectMaterial=null,this._blurMaterial=null,this._ggxMaterial=null}fromScene(e,t=0,i=.1,s=100,r={}){let{size:o=256,position:a=ov}=r;Gh=this._renderer.getRenderTarget(),Vh=this._renderer.getActiveCubeFace(),Oh=this._renderer.getActiveMipmapLevel(),kh=this._renderer.xr.enabled,this._renderer.xr.enabled=!1,this._setSize(o);let l=this._allocateTargets();return l.depthBuffer=!0,this._sceneToCubeUV(e,i,s,l,a),t>0&&this._blur(l,0,0,t),this._applyPMREM(l),this._cleanup(l),l}fromEquirectangular(e,t=null){return this._fromTexture(e,t)}fromCubemap(e,t=null){return this._fromTexture(e,t)}compileCubemapShader(){this._cubemapMaterial===null&&(this._cubemapMaterial=lm(),this._compileMaterial(this._cubemapMaterial))}compileEquirectangularShader(){this._equirectMaterial===null&&(this._equirectMaterial=am(),this._compileMaterial(this._equirectMaterial))}dispose(){this._dispose(),this._cubemapMaterial!==null&&this._cubemapMaterial.dispose(),this._equirectMaterial!==null&&this._equirectMaterial.dispose(),this._backgroundBox!==null&&(this._backgroundBox.geometry.dispose(),this._backgroundBox.material.dispose())}_setSize(e){this._lodMax=Math.floor(Math.log2(e)),this._cubeSize=Math.pow(2,this._lodMax)}_dispose(){this._blurMaterial!==null&&this._blurMaterial.dispose(),this._ggxMaterial!==null&&this._ggxMaterial.dispose(),this._pingPongRenderTarget!==null&&this._pingPongRenderTarget.dispose();for(let e=0;e<this._lodMeshes.length;e++)this._lodMeshes[e].geometry.dispose()}_cleanup(e){this._renderer.setRenderTarget(Gh,Vh,Oh),this._renderer.xr.enabled=kh,e.scissorTest=!1,Ur(e,0,0,e.width,e.height)}_fromTexture(e,t){e.mapping===hi||e.mapping===Bi?this._setSize(e.image.length===0?16:e.image[0].width||e.image[0].image.width):this._setSize(e.image.width/4),Gh=this._renderer.getRenderTarget(),Vh=this._renderer.getActiveCubeFace(),Oh=this._renderer.getActiveMipmapLevel(),kh=this._renderer.xr.enabled,this._renderer.xr.enabled=!1;let i=t||this._allocateTargets();return this._textureToCubeUV(e,i),this._applyPMREM(i),this._cleanup(i),i}_allocateTargets(){let e=3*Math.max(this._cubeSize,112),t=4*this._cubeSize,i={magFilter:Bt,minFilter:Bt,generateMipmaps:!1,type:Sn,format:gn,colorSpace:Wi,depthBuffer:!1},s=om(e,t,i);if(this._pingPongRenderTarget===null||this._pingPongRenderTarget.width!==e||this._pingPongRenderTarget.height!==t){this._pingPongRenderTarget!==null&&this._dispose(),this._pingPongRenderTarget=om(e,t,i);let{_lodMax:r}=this;({lodMeshes:this._lodMeshes,sizeLods:this._sizeLods}=av(r)),this._blurMaterial=cv(r,e,t),this._ggxMaterial=lv(r,e,t)}return s}_compileMaterial(e){let t=new fn(new dn,e);this._renderer.compile(t,ga)}_sceneToCubeUV(e,t,i,s,r){let l=new Kt(90,1,t,i),c=[1,-1,1,1,1,1],u=[1,1,1,-1,-1,-1],d=this._renderer,h=d.autoClear,f=d.toneMapping;d.getClearColor(rm),d.toneMapping=pn,d.autoClear=!1,d.state.buffers.depth.getReversed()&&(d.setRenderTarget(s),d.clearDepth(),d.setRenderTarget(null)),this._backgroundBox===null&&(this._backgroundBox=new fn(new ys,new _r({name:"PMREM.Background",side:Dt,depthWrite:!1,depthTest:!1})));let y=this._backgroundBox,m=y.material,p=!1,E=e.background;E?E.isColor&&(m.color.copy(E),e.background=null,p=!0):(m.color.copy(rm),p=!0);for(let w=0;w<6;w++){let S=w%3;S===0?(l.up.set(0,c[w],0),l.position.set(r.x,r.y,r.z),l.lookAt(r.x+u[w],r.y,r.z)):S===1?(l.up.set(0,0,c[w]),l.position.set(r.x,r.y,r.z),l.lookAt(r.x,r.y+u[w],r.z)):(l.up.set(0,c[w],0),l.position.set(r.x,r.y,r.z),l.lookAt(r.x,r.y,r.z+u[w]));let v=this._cubeSize;Ur(s,S*v,w>2?v:0,v,v),d.setRenderTarget(s),p&&d.render(y,l),d.render(e,l)}d.toneMapping=f,d.autoClear=h,e.background=E}_textureToCubeUV(e,t){let i=this._renderer,s=e.mapping===hi||e.mapping===Bi;s?(this._cubemapMaterial===null&&(this._cubemapMaterial=lm()),this._cubemapMaterial.uniforms.flipEnvMap.value=e.isRenderTargetTexture===!1?-1:1):this._equirectMaterial===null&&(this._equirectMaterial=am());let r=s?this._cubemapMaterial:this._equirectMaterial,o=this._lodMeshes[0];o.material=r;let a=r.uniforms;a.envMap.value=e;let l=this._cubeSize;Ur(t,0,0,3*l,2*l),i.setRenderTarget(t),i.render(o,ga)}_applyPMREM(e){let t=this._renderer,i=t.autoClear;t.autoClear=!1;let s=this._lodMeshes.length;for(let r=1;r<s;r++)this._applyGGXFilter(e,r-1,r);t.autoClear=i}_applyGGXFilter(e,t,i){let s=this._renderer,r=this._pingPongRenderTarget,o=this._ggxMaterial,a=this._lodMeshes[i];a.material=o;let l=o.uniforms,c=i/(this._lodMeshes.length-1),u=t/(this._lodMeshes.length-1),d=Math.sqrt(c*c-u*u),h=c*1.25,f=d*h,{_lodMax:g}=this,y=this._sizeLods[i],m=3*y*(i>g-Dr?i-g+Dr:0),p=4*(this._cubeSize-y);l.envMap.value=e.texture,l.roughness.value=f,l.mipInt.value=g-t,Ur(r,m,p,3*y,2*y),s.setRenderTarget(r),s.render(a,ga),l.envMap.value=r.texture,l.roughness.value=0,l.mipInt.value=g-i,Ur(e,m,p,3*y,2*y),s.setRenderTarget(e),s.render(a,ga)}_blur(e,t,i,s){let r=this._pingPongRenderTarget,o=Math.min(s,Math.PI)/Math.SQRT2;this._blurPass(e,r,t,i,o),this._blurPass(r,e,i,i,o)}_blurPass(e,t,i,s,r){let o=this._renderer,a=this._blurMaterial,l=this._lodMeshes[s];l.material=a;let c=a.uniforms;c.envMap.value=e.texture,c.sigma.value=r,c.mipInt.value=this._lodMax-i;let u=this._sizeLods[s],d=3*u*(s>this._lodMax-Dr?s-this._lodMax+Dr:0),h=4*(this._cubeSize-u);Ur(t,d,h,3*u,2*u),o.setRenderTarget(t),o.render(l,ga)}};function av(n){let e=[],t=[],i=n,s=n-Dr+1+iv;for(let r=0;r<s;r++){let o=Math.pow(2,i);e.push(o);let a=1/(o-2),l=-a,c=1+a,u=[l,l,c,l,c,c,l,l,c,c,l,c],d=6,h=6,f=3,g=new Float32Array(f*h*d),y=new Float32Array(f*h*d);for(let p=0;p<d;p++){let E=p%3*2/3-1,w=p>2?0:-1,S=[E,w,0,E+2/3,w,0,E+2/3,w+1,0,E,w,0,E+2/3,w+1,0,E,w+1,0];g.set(S,f*h*p);for(let v=0;v<h;v++){let A=u[v*2]*2-1,R=u[v*2+1]*2-1;p===0?Ns.set(1,R,A):p===1?Ns.set(-A,1,-R):p===2?Ns.set(-A,R,1):p===3?Ns.set(-1,R,-A):p===4?Ns.set(-A,-1,R):Ns.set(A,R,-1),Ns.toArray(y,(p*h+v)*f)}}let m=new dn;m.setAttribute("position",new Xt(g,f)),m.setAttribute("outputDirection",new Xt(y,f)),t.push(new fn(m,null)),i>Dr&&i--}return{lodMeshes:t,sizeLods:e}}function om(n,e,t){let i=new _n(n,e,t);return i.texture.mapping=Er,i.texture.name="PMREM.cubeUv",i.scissorTest=!0,i}function Ur(n,e,t,i,s){n.viewport.set(e,t,i,s),n.scissor.set(e,t,i,s)}function lv(n,e,t){return new Cn({name:"PMREMGGXConvolution",defines:{GGX_SAMPLES:rv,CUBEUV_TEXEL_WIDTH:1/e,CUBEUV_TEXEL_HEIGHT:1/t,CUBEUV_MAX_MIP:`${n}.0`},uniforms:{envMap:{value:null},roughness:{value:0},mipInt:{value:0}},vertexShader:yc(),fragmentShader:`

			precision highp float;
			precision highp int;

			varying vec3 vOutputDirection;

			uniform sampler2D envMap;
			uniform float roughness;
			uniform float mipInt;

			#define ENVMAP_TYPE_CUBE_UV
			#include <cube_uv_reflection_fragment>

			#define PI 3.14159265359

			// Van der Corput radical inverse
			float radicalInverse_VdC(uint bits) {
				bits = (bits << 16u) | (bits >> 16u);
				bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
				bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
				bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
				bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
				return float(bits) * 2.3283064365386963e-10; // / 0x100000000
			}

			// Hammersley sequence
			vec2 hammersley(uint i, uint N) {
				return vec2(float(i) / float(N), radicalInverse_VdC(i));
			}

			// GGX VNDF importance sampling (Eric Heitz 2018)
			// "Sampling the GGX Distribution of Visible Normals"
			// https://jcgt.org/published/0007/04/01/
			vec3 importanceSampleGGX_VNDF(vec2 Xi, vec3 V, float roughness) {
				float alpha = roughness * roughness;

				// Section 4.1: Orthonormal basis
				vec3 T1 = vec3(1.0, 0.0, 0.0);
				vec3 T2 = cross(V, T1);

				// Section 4.2: Parameterization of projected area
				float r = sqrt(Xi.x);
				float phi = 2.0 * PI * Xi.y;
				float t1 = r * cos(phi);
				float t2 = r * sin(phi);
				float s = 0.5 * (1.0 + V.z);
				t2 = (1.0 - s) * sqrt(1.0 - t1 * t1) + s * t2;

				// Section 4.3: Reprojection onto hemisphere
				vec3 Nh = t1 * T1 + t2 * T2 + sqrt(max(0.0, 1.0 - t1 * t1 - t2 * t2)) * V;

				// Section 3.4: Transform back to ellipsoid configuration
				return normalize(vec3(alpha * Nh.x, alpha * Nh.y, max(0.0, Nh.z)));
			}

			void main() {
				vec3 N = normalize(vOutputDirection);
				vec3 V = N; // Assume view direction equals normal for pre-filtering

				vec3 prefilteredColor = vec3(0.0);
				float totalWeight = 0.0;

				// For very low roughness, just sample the environment directly
				if (roughness < 0.001) {
					gl_FragColor = vec4(bilinearCubeUV(envMap, N, mipInt), 1.0);
					return;
				}

				// Tangent space basis for VNDF sampling
				vec3 up = abs(N.z) < 0.999 ? vec3(0.0, 0.0, 1.0) : vec3(1.0, 0.0, 0.0);
				vec3 tangent = normalize(cross(up, N));
				vec3 bitangent = cross(N, tangent);

				for(uint i = 0u; i < uint(GGX_SAMPLES); i++) {
					vec2 Xi = hammersley(i, uint(GGX_SAMPLES));

					// For PMREM, V = N, so in tangent space V is always (0, 0, 1)
					vec3 H_tangent = importanceSampleGGX_VNDF(Xi, vec3(0.0, 0.0, 1.0), roughness);

					// Transform H back to world space
					vec3 H = normalize(tangent * H_tangent.x + bitangent * H_tangent.y + N * H_tangent.z);
					vec3 L = normalize(2.0 * dot(V, H) * H - V);

					float NdotL = max(dot(N, L), 0.0);

					if(NdotL > 0.0) {
						// Sample environment at fixed mip level
						// VNDF importance sampling handles the distribution filtering
						vec3 sampleColor = bilinearCubeUV(envMap, L, mipInt);

						// Weight by NdotL for the split-sum approximation
						// VNDF PDF naturally accounts for the visible microfacet distribution
						prefilteredColor += sampleColor * NdotL;
						totalWeight += NdotL;
					}
				}

				if (totalWeight > 0.0) {
					prefilteredColor = prefilteredColor / totalWeight;
				}

				gl_FragColor = vec4(prefilteredColor, 1.0);
			}
		`,blending:qn,depthTest:!1,depthWrite:!1})}function cv(n,e,t){return new Cn({name:"SphericalGaussianBlur",defines:{SAMPLES:sv,CUBEUV_TEXEL_WIDTH:1/e,CUBEUV_TEXEL_HEIGHT:1/t,CUBEUV_MAX_MIP:`${n}.0`},uniforms:{envMap:{value:null},sigma:{value:0},mipInt:{value:0}},vertexShader:yc(),fragmentShader:`

			precision highp float;
			precision highp int;

			varying vec3 vOutputDirection;

			uniform sampler2D envMap;
			uniform float sigma;
			uniform float mipInt;

			#define ENVMAP_TYPE_CUBE_UV
			#include <cube_uv_reflection_fragment>

			#define PI 3.14159265359
			#define GOLDEN_ANGLE 2.39996322973

			void main() {

				if ( sigma == 0.0 ) {

					gl_FragColor = vec4( bilinearCubeUV( envMap, vOutputDirection, mipInt ), 1.0 );
					return;

				}

				vec3 outputDirection = normalize( vOutputDirection );

				vec3 up = abs( outputDirection.z ) < 0.999 ? vec3( 0.0, 0.0, 1.0 ) : vec3( 1.0, 0.0, 0.0 );
				vec3 tangent = normalize( cross( up, outputDirection ) );
				vec3 bitangent = cross( outputDirection, tangent );

				// Truncate the kernel at three standard deviations or at the antipode.
				float thetaMax = min( 3.0 * sigma, PI );
				float truncation = 1.0 - exp( - 0.5 * thetaMax * thetaMax / ( sigma * sigma ) );

				vec3 accumColor = vec3( 0.0 );
				float accumWeight = 0.0;

				for ( int i = 0; i < SAMPLES; i ++ ) {

					// Stratified inverse-CDF sampling of the Gaussian, placed on a golden-angle spiral.
					float stratum = ( float( i ) + 0.5 ) / float( SAMPLES );
					float theta = sigma * sqrt( - 2.0 * log( 1.0 - stratum * truncation ) );
					float phi = float( i ) * GOLDEN_ANGLE;

					vec3 offset = cos( phi ) * tangent + sin( phi ) * bitangent;
					vec3 sampleDirection = cos( theta ) * outputDirection + sin( theta ) * offset;

					// Correct the planar sample density to solid angle.
					float weight = sin( theta ) / theta;

					accumColor += weight * bilinearCubeUV( envMap, sampleDirection, mipInt );
					accumWeight += weight;

				}

				gl_FragColor = vec4( accumColor / accumWeight, 1.0 );

			}
		`,blending:qn,depthTest:!1,depthWrite:!1})}function am(){return new Cn({name:"EquirectangularToCubeUV",uniforms:{envMap:{value:null}},vertexShader:yc(),fragmentShader:`

			precision mediump float;
			precision mediump int;

			varying vec3 vOutputDirection;

			uniform sampler2D envMap;

			#include <common>

			void main() {

				vec3 outputDirection = normalize( vOutputDirection );
				vec2 uv = equirectUv( outputDirection );

				gl_FragColor = vec4( texture2D ( envMap, uv ).rgb, 1.0 );

			}
		`,blending:qn,depthTest:!1,depthWrite:!1})}function lm(){return new Cn({name:"CubemapToCubeUV",uniforms:{envMap:{value:null},flipEnvMap:{value:-1}},vertexShader:yc(),fragmentShader:`

			precision mediump float;
			precision mediump int;

			uniform float flipEnvMap;

			varying vec3 vOutputDirection;

			uniform samplerCube envMap;

			void main() {

				gl_FragColor = textureCube( envMap, vec3( flipEnvMap * vOutputDirection.x, vOutputDirection.yz ) );

			}
		`,blending:qn,depthTest:!1,depthWrite:!1})}function yc(){return`

		precision mediump float;
		precision mediump int;

		attribute vec3 outputDirection;

		varying vec3 vOutputDirection;

		void main() {

			vOutputDirection = outputDirection;
			gl_Position = vec4( position, 1.0 );

		}
	`}var gc=class extends _n{constructor(e=1,t={}){super(e,e,t),this.isWebGLCubeRenderTarget=!0;let i={width:e,height:e,depth:1},s=[i,i,i,i,i,i];this.texture=new xs(s),this._setTextureOptions(t),this.texture.isRenderTargetTexture=!0}fromEquirectangularTexture(e,t){this.texture.type=t.type,this.texture.colorSpace=t.colorSpace,this.texture.generateMipmaps=t.generateMipmaps,this.texture.minFilter=t.minFilter,this.texture.magFilter=t.magFilter;let i={uniforms:{tEquirect:{value:null}},vertexShader:`

				varying vec3 vWorldDirection;

				vec3 transformDirection( in vec3 dir, in mat4 matrix ) {

					return normalize( ( matrix * vec4( dir, 0.0 ) ).xyz );

				}

				void main() {

					vWorldDirection = transformDirection( position, modelMatrix );

					#include <begin_vertex>
					#include <project_vertex>

				}
			`,fragmentShader:`

				uniform sampler2D tEquirect;

				varying vec3 vWorldDirection;

				#include <common>

				void main() {

					vec3 direction = normalize( vWorldDirection );

					vec2 sampleUV = equirectUv( direction );

					gl_FragColor = texture2D( tEquirect, sampleUV );

				}
			`},s=new ys(5,5,5),r=new Cn({name:"CubemapFromEquirect",uniforms:Cs(i.uniforms),vertexShader:i.vertexShader,fragmentShader:i.fragmentShader,side:Dt,blending:qn});r.uniforms.tEquirect.value=t;let o=new fn(s,r),a=t.minFilter;return t.minFilter===di&&(t.minFilter=Bt),new Mo(1,10,this).update(e,o),t.minFilter=a,o.geometry.dispose(),o.material.dispose(),this}clear(e,t=!0,i=!0,s=!0){let r=e.getRenderTarget();for(let o=0;o<6;o++)e.setRenderTarget(this,o),e.clear(t,i,s);e.setRenderTarget(r)}};function uv(n){let e=new WeakMap,t=new WeakMap,i=null;function s(h,f=!1){return h==null?null:f?o(h):r(h)}function r(h){if(h&&h.isTexture){let f=h.mapping;if(f===vs||f===No)if(e.has(h)){let g=e.get(h).texture;return a(g,h.mapping)}else{let g=h.image;if(g&&g.height>0){let y=new gc(g.height);return y.fromEquirectangularTexture(n,h),e.set(h,y),h.addEventListener("dispose",c),a(y.texture,h.mapping)}else return null}}return h}function o(h){if(h&&h.isTexture){let f=h.mapping,g=f===vs||f===No,y=f===hi||f===Bi;if(g||y){let m=t.get(h),p=m!==void 0?m.texture.pmremVersion:0;if(h.isRenderTargetTexture&&h.pmremVersion!==p)return i===null&&(i=new mc(n)),m=g?i.fromEquirectangular(h,m):i.fromCubemap(h,m),m.texture.pmremVersion=h.pmremVersion,t.set(h,m),m.texture;if(m!==void 0)return m.texture;{let E=h.image;return g&&E&&E.height>0||y&&E&&l(E)?(i===null&&(i=new mc(n)),m=g?i.fromEquirectangular(h):i.fromCubemap(h),m.texture.pmremVersion=h.pmremVersion,t.set(h,m),h.addEventListener("dispose",u),m.texture):null}}}return h}function a(h,f){return f===vs?h.mapping=hi:f===No&&(h.mapping=Bi),h}function l(h){let f=0,g=6;for(let y=0;y<g;y++)h[y]!==void 0&&f++;return f===g}function c(h){let f=h.target;f.removeEventListener("dispose",c);let g=e.get(f);g!==void 0&&(e.delete(f),g.dispose())}function u(h){let f=h.target;f.removeEventListener("dispose",u);let g=t.get(f);g!==void 0&&(t.delete(f),g.dispose())}function d(){e=new WeakMap,t=new WeakMap,i!==null&&(i.dispose(),i=null)}return{get:s,dispose:d}}function hv(n){let e={};function t(i){if(e[i]!==void 0)return e[i];let s=n.getExtension(i);return e[i]=s,s}return{has:function(i){return t(i)!==null},init:function(){t("EXT_color_buffer_float"),t("WEBGL_clip_cull_distance"),t("OES_texture_float_linear"),t("EXT_color_buffer_half_float"),t("WEBGL_multisampled_render_to_texture"),t("WEBGL_render_shared_exponent")},get:function(i){let s=t(i);return s===null&&Zn("WebGLRenderer: "+i+" extension not supported."),s}}}function dv(n,e,t,i){let s={},r=new WeakMap;function o(d){let h=d.target;h.index!==null&&e.remove(h.index);for(let g in h.attributes)e.remove(h.attributes[g]);h.removeEventListener("dispose",o),delete s[h.id];let f=r.get(h);f&&(e.remove(f),r.delete(h)),i.releaseStatesOfGeometry(h),h.isInstancedBufferGeometry===!0&&delete h._maxInstanceCount,t.memory.geometries--}function a(d,h){return s[h.id]===!0||(h.addEventListener("dispose",o),s[h.id]=!0,t.memory.geometries++),h}function l(d){let h=d.attributes;for(let f in h)e.update(h[f],n.ARRAY_BUFFER)}function c(d){let h=[],f=d.index,g=d.attributes.position,y=0;if(g===void 0)return;if(f!==null){let E=f.array;y=f.version;for(let w=0,S=E.length;w<S;w+=3){let v=E[w+0],A=E[w+1],R=E[w+2];h.push(v,A,A,R,R,v)}}else{let E=g.array;y=g.version;for(let w=0,S=E.length/3-1;w<S;w+=3){let v=w+0,A=w+1,R=w+2;h.push(v,A,A,R,R,v)}}let m=new(g.count>=65535?xr:gr)(h,1);m.version=y;let p=r.get(d);p&&e.remove(p),r.set(d,m)}function u(d){let h=r.get(d);if(h){let f=d.index;f!==null&&h.version<f.version&&c(d)}else c(d);return r.get(d)}return{get:a,update:l,getWireframeAttribute:u}}function fv(n,e,t){let i;function s(d){i=d}let r,o;function a(d){r=d.type,o=d.bytesPerElement}function l(d,h){n.drawElements(i,h,r,d*o),t.update(h,i,1)}function c(d,h,f){f!==0&&(n.drawElementsInstanced(i,h,r,d*o,f),t.update(h,i,f))}function u(d,h,f){if(f===0)return;e.get("WEBGL_multi_draw").multiDrawElementsWEBGL(i,h,0,r,d,0,f);let y=0;for(let m=0;m<f;m++)y+=h[m];t.update(y,i,1)}this.setMode=s,this.setIndex=a,this.render=l,this.renderInstances=c,this.renderMultiDraw=u}function pv(n){let e={geometries:0,textures:0},t={frame:0,calls:0,triangles:0,points:0,lines:0};function i(r,o,a){switch(t.calls++,o){case n.TRIANGLES:t.triangles+=a*(r/3);break;case n.LINES:t.lines+=a*(r/2);break;case n.LINE_STRIP:t.lines+=a*(r-1);break;case n.LINE_LOOP:t.lines+=a*r;break;case n.POINTS:t.points+=a*r;break;default:_e("WebGLInfo: Unknown draw mode:",o);break}}function s(){t.calls=0,t.triangles=0,t.points=0,t.lines=0}return{memory:e,render:t,programs:null,autoReset:!0,reset:s,update:i}}function mv(n,e,t){let i=new WeakMap,s=new dt;function r(o,a,l){let c=o.morphTargetInfluences,u=a.morphAttributes.position||a.morphAttributes.normal||a.morphAttributes.color,d=u!==void 0?u.length:0,h=i.get(a);if(h===void 0||h.count!==d){let M=function(){R.dispose(),i.delete(a),a.removeEventListener("dispose",M)};h!==void 0&&h.texture.dispose();let f=a.morphAttributes.position!==void 0,g=a.morphAttributes.normal!==void 0,y=a.morphAttributes.color!==void 0,m=a.morphAttributes.position||[],p=a.morphAttributes.normal||[],E=a.morphAttributes.color||[],w=0;f===!0&&(w=1),g===!0&&(w=2),y===!0&&(w=3);let S=a.attributes.position.count*w,v=1;S>e.maxTextureSize&&(v=Math.ceil(S/e.maxTextureSize),S=e.maxTextureSize);let A=new Float32Array(S*v*4*d),R=new fr(A,S,v,d);R.type=Dn,R.needsUpdate=!0;let b=w*4;for(let B=0;B<d;B++){let D=m[B],k=p[B],H=E[B],F=S*v*4*B;for(let O=0;O<D.count;O++){let q=O*b;f===!0&&(s.fromBufferAttribute(D,O),A[F+q+0]=s.x,A[F+q+1]=s.y,A[F+q+2]=s.z,A[F+q+3]=0),g===!0&&(s.fromBufferAttribute(k,O),A[F+q+4]=s.x,A[F+q+5]=s.y,A[F+q+6]=s.z,A[F+q+7]=0),y===!0&&(s.fromBufferAttribute(H,O),A[F+q+8]=s.x,A[F+q+9]=s.y,A[F+q+10]=s.z,A[F+q+11]=H.itemSize===4?s.w:1)}}h={count:d,texture:R,size:new ze(S,v)},i.set(a,h),a.addEventListener("dispose",M)}if(o.isInstancedMesh===!0&&o.morphTexture!==null)l.getUniforms().setValue(n,"morphTexture",o.morphTexture,t);else{let f=0;for(let y=0;y<c.length;y++)f+=c[y];let g=a.morphTargetsRelative?1:1-f;l.getUniforms().setValue(n,"morphTargetBaseInfluence",g),l.getUniforms().setValue(n,"morphTargetInfluences",c)}l.getUniforms().setValue(n,"morphTargetsTexture",h.texture,t),l.getUniforms().setValue(n,"morphTargetsTextureSize",h.size)}return{update:r}}function gv(n,e,t,i,s){let r=new WeakMap;function o(c){let u=s.render.frame,d=c.geometry,h=e.get(c,d);if(r.get(h)!==u&&(e.update(h),r.set(h,u)),c.isInstancedMesh&&(c.hasEventListener("dispose",l)===!1&&c.addEventListener("dispose",l),r.get(c)!==u&&(t.update(c.instanceMatrix,n.ARRAY_BUFFER),c.instanceColor!==null&&t.update(c.instanceColor,n.ARRAY_BUFFER),r.set(c,u))),c.isSkinnedMesh){let f=c.skeleton;r.get(f)!==u&&(f.update(),r.set(f,u))}return h}function a(){r=new WeakMap}function l(c){let u=c.target;u.removeEventListener("dispose",l),i.releaseStatesOfObject(u),t.remove(u.instanceMatrix),u.instanceColor!==null&&t.remove(u.instanceColor)}return{update:o,dispose:a}}var xv={[Jl]:"LINEAR_TONE_MAPPING",[ql]:"REINHARD_TONE_MAPPING",[Ql]:"CINEON_TONE_MAPPING",[jl]:"ACES_FILMIC_TONE_MAPPING",[ec]:"AGX_TONE_MAPPING",[Tr]:"NEUTRAL_TONE_MAPPING",[Th]:"CUSTOM_TONE_MAPPING"};function yv(n,e,t,i,s,r){let o=new _n(e,t,{type:n,depthBuffer:s,stencilBuffer:r,samples:i?4:0,storeMultisampledDepthBuffer:!1,storeMultisampledStencilBuffer:!1,resolveDepthBuffer:!1,resolveStencilBuffer:!1}),a=null,l=null,c=new dn;c.setAttribute("position",new nn([-1,3,0,-1,-1,0,3,-1,0],3)),c.setAttribute("uv",new nn([0,2,0,0,2,0],2));let u=new Cl({uniforms:{tDiffuse:{value:null}},vertexShader:`
			precision highp float;

			uniform mat4 modelViewMatrix;
			uniform mat4 projectionMatrix;

			attribute vec3 position;
			attribute vec2 uv;

			varying vec2 vUv;

			void main() {
				vUv = uv;
				gl_Position = projectionMatrix * modelViewMatrix * vec4( position, 1.0 );
			}`,fragmentShader:`
			precision highp float;

			uniform sampler2D tDiffuse;

			varying vec2 vUv;

			#include <tonemapping_pars_fragment>
			#include <colorspace_pars_fragment>

			void main() {
				gl_FragColor = texture2D( tDiffuse, vUv );

				#ifdef LINEAR_TONE_MAPPING
					gl_FragColor.rgb = LinearToneMapping( gl_FragColor.rgb );
				#elif defined( REINHARD_TONE_MAPPING )
					gl_FragColor.rgb = ReinhardToneMapping( gl_FragColor.rgb );
				#elif defined( CINEON_TONE_MAPPING )
					gl_FragColor.rgb = CineonToneMapping( gl_FragColor.rgb );
				#elif defined( ACES_FILMIC_TONE_MAPPING )
					gl_FragColor.rgb = ACESFilmicToneMapping( gl_FragColor.rgb );
				#elif defined( AGX_TONE_MAPPING )
					gl_FragColor.rgb = AgXToneMapping( gl_FragColor.rgb );
				#elif defined( NEUTRAL_TONE_MAPPING )
					gl_FragColor.rgb = NeutralToneMapping( gl_FragColor.rgb );
				#elif defined( CUSTOM_TONE_MAPPING )
					gl_FragColor.rgb = CustomToneMapping( gl_FragColor.rgb );
				#endif

				#ifdef SRGB_TRANSFER
					gl_FragColor = sRGBTransferOETF( gl_FragColor );
				#endif
			}`,depthTest:!1,depthWrite:!1}),d=new fn(c,u),h=new _s(-1,1,1,-1,0,1),f=null,g=null,y=!1,m,p=null,E=[],w=!1;this.setSize=function(S,v){o.setSize(S,v),a!==null&&a.setSize(S,v),l!==null&&l.setSize(S,v);for(let A=0;A<E.length;A++){let R=E[A];R.setSize&&R.setSize(S,v)}},this.setEffects=function(S){E=S,w=E.length>0&&E[0].isRenderPass===!0;let v=o.width,A=o.height;E.length>0&&a===null&&(a=new _n(v,A,{type:Sn,depthBuffer:!1,stencilBuffer:!1}),l=new _n(v,A,{type:Sn,depthBuffer:!1,stencilBuffer:!1}));for(let R=0;R<E.length;R++){let b=E[R];b.setSize&&b.setSize(v,A)}},this.begin=function(S,v){if(y||S.toneMapping===pn&&E.length===0)return!1;if(p=v,v!==null){let A=v.width,R=v.height;(o.width!==A||o.height!==R)&&this.setSize(A,R)}return w===!1&&S.setRenderTarget(o),m=S.toneMapping,S.toneMapping=pn,!0},this.hasRenderPass=function(){return w},this.end=function(S,v){S.toneMapping=m,y=!0;let A=o,R=a;for(let b=0;b<E.length;b++){let M=E[b];M.enabled!==!1&&(M.render(S,R,A,v),M.needsSwap!==!1&&(A=R,R=R===a?l:a))}if(f!==S.outputColorSpace||g!==S.toneMapping){f=S.outputColorSpace,g=S.toneMapping,u.defines={},Ve.getTransfer(f)===nt&&(u.defines.SRGB_TRANSFER="");let b=xv[g];b&&(u.defines[b]=""),u.needsUpdate=!0}u.uniforms.tDiffuse.value=A.texture,S.setRenderTarget(p),S.render(d,h),p=null,y=!1},this.isCompositing=function(){return y},this.dispose=function(){o.dispose(),a!==null&&a.dispose(),l!==null&&l.dispose(),c.dispose(),u.dispose()}}var Cm=new $t,Hh=new $n(1,1),Nm=new fr,Im=new El,Bm=new xs,cm=[],um=[],hm=new Float32Array(16),dm=new Float32Array(9),fm=new Float32Array(4);function Gr(n,e,t){let i=n[0];if(i<=0||i>0)return n;let s=e*t,r=cm[s];if(r===void 0&&(r=new Float32Array(s),cm[s]=r),e!==0){i.toArray(r,0);for(let o=1,a=0;o!==e;++o)a+=t,n[o].toArray(r,a)}return r}function kt(n,e){if(n.length!==e.length)return!1;for(let t=0,i=n.length;t<i;t++)if(n[t]!==e[t])return!1;return!0}function zt(n,e){for(let t=0,i=e.length;t<i;t++)n[t]=e[t]}function _c(n,e){let t=um[e];t===void 0&&(t=new Int32Array(e),um[e]=t);for(let i=0;i!==e;++i)t[i]=n.allocateTextureUnit();return t}function _v(n,e){let t=this.cache;t[0]!==e&&(n.uniform1f(this.addr,e),t[0]=e)}function bv(n,e){let t=this.cache;if(e.x!==void 0)(t[0]!==e.x||t[1]!==e.y)&&(n.uniform2f(this.addr,e.x,e.y),t[0]=e.x,t[1]=e.y);else{if(kt(t,e))return;n.uniform2fv(this.addr,e),zt(t,e)}}function Sv(n,e){let t=this.cache;if(e.x!==void 0)(t[0]!==e.x||t[1]!==e.y||t[2]!==e.z)&&(n.uniform3f(this.addr,e.x,e.y,e.z),t[0]=e.x,t[1]=e.y,t[2]=e.z);else if(e.r!==void 0)(t[0]!==e.r||t[1]!==e.g||t[2]!==e.b)&&(n.uniform3f(this.addr,e.r,e.g,e.b),t[0]=e.r,t[1]=e.g,t[2]=e.b);else{if(kt(t,e))return;n.uniform3fv(this.addr,e),zt(t,e)}}function vv(n,e){let t=this.cache;if(e.x!==void 0)(t[0]!==e.x||t[1]!==e.y||t[2]!==e.z||t[3]!==e.w)&&(n.uniform4f(this.addr,e.x,e.y,e.z,e.w),t[0]=e.x,t[1]=e.y,t[2]=e.z,t[3]=e.w);else{if(kt(t,e))return;n.uniform4fv(this.addr,e),zt(t,e)}}function Av(n,e){let t=this.cache,i=e.elements;if(i===void 0){if(kt(t,e))return;n.uniformMatrix2fv(this.addr,!1,e),zt(t,e)}else{if(kt(t,i))return;fm.set(i),n.uniformMatrix2fv(this.addr,!1,fm),zt(t,i)}}function Tv(n,e){let t=this.cache,i=e.elements;if(i===void 0){if(kt(t,e))return;n.uniformMatrix3fv(this.addr,!1,e),zt(t,e)}else{if(kt(t,i))return;dm.set(i),n.uniformMatrix3fv(this.addr,!1,dm),zt(t,i)}}function Ev(n,e){let t=this.cache,i=e.elements;if(i===void 0){if(kt(t,e))return;n.uniformMatrix4fv(this.addr,!1,e),zt(t,e)}else{if(kt(t,i))return;hm.set(i),n.uniformMatrix4fv(this.addr,!1,hm),zt(t,i)}}function Mv(n,e){let t=this.cache;t[0]!==e&&(n.uniform1i(this.addr,e),t[0]=e)}function Rv(n,e){let t=this.cache;if(e.x!==void 0)(t[0]!==e.x||t[1]!==e.y)&&(n.uniform2i(this.addr,e.x,e.y),t[0]=e.x,t[1]=e.y);else{if(kt(t,e))return;n.uniform2iv(this.addr,e),zt(t,e)}}function wv(n,e){let t=this.cache;if(e.x!==void 0)(t[0]!==e.x||t[1]!==e.y||t[2]!==e.z)&&(n.uniform3i(this.addr,e.x,e.y,e.z),t[0]=e.x,t[1]=e.y,t[2]=e.z);else{if(kt(t,e))return;n.uniform3iv(this.addr,e),zt(t,e)}}function Cv(n,e){let t=this.cache;if(e.x!==void 0)(t[0]!==e.x||t[1]!==e.y||t[2]!==e.z||t[3]!==e.w)&&(n.uniform4i(this.addr,e.x,e.y,e.z,e.w),t[0]=e.x,t[1]=e.y,t[2]=e.z,t[3]=e.w);else{if(kt(t,e))return;n.uniform4iv(this.addr,e),zt(t,e)}}function Nv(n,e){let t=this.cache;t[0]!==e&&(n.uniform1ui(this.addr,e),t[0]=e)}function Iv(n,e){let t=this.cache;if(e.x!==void 0)(t[0]!==e.x||t[1]!==e.y)&&(n.uniform2ui(this.addr,e.x,e.y),t[0]=e.x,t[1]=e.y);else{if(kt(t,e))return;n.uniform2uiv(this.addr,e),zt(t,e)}}function Bv(n,e){let t=this.cache;if(e.x!==void 0)(t[0]!==e.x||t[1]!==e.y||t[2]!==e.z)&&(n.uniform3ui(this.addr,e.x,e.y,e.z),t[0]=e.x,t[1]=e.y,t[2]=e.z);else{if(kt(t,e))return;n.uniform3uiv(this.addr,e),zt(t,e)}}function Fv(n,e){let t=this.cache;if(e.x!==void 0)(t[0]!==e.x||t[1]!==e.y||t[2]!==e.z||t[3]!==e.w)&&(n.uniform4ui(this.addr,e.x,e.y,e.z,e.w),t[0]=e.x,t[1]=e.y,t[2]=e.z,t[3]=e.w);else{if(kt(t,e))return;n.uniform4uiv(this.addr,e),zt(t,e)}}function Lv(n,e,t){let i=this.cache,s=t.allocateTextureUnit();i[0]!==s&&(n.uniform1i(this.addr,s),i[0]=s);let r;this.type===n.SAMPLER_2D_SHADOW?(Hh.compareFunction=t.isReversedDepthBuffer()?$i:ws,r=Hh):r=Cm,t.setTexture2D(e||r,s)}function Uv(n,e,t){let i=this.cache,s=t.allocateTextureUnit();i[0]!==s&&(n.uniform1i(this.addr,s),i[0]=s),t.setTexture3D(e||Im,s)}function Dv(n,e,t){let i=this.cache,s=t.allocateTextureUnit();i[0]!==s&&(n.uniform1i(this.addr,s),i[0]=s),t.setTextureCube(e||Bm,s)}function Pv(n,e,t){let i=this.cache,s=t.allocateTextureUnit();i[0]!==s&&(n.uniform1i(this.addr,s),i[0]=s),t.setTexture2DArray(e||Nm,s)}function Gv(n){switch(n){case 5126:return _v;case 35664:return bv;case 35665:return Sv;case 35666:return vv;case 35674:return Av;case 35675:return Tv;case 35676:return Ev;case 5124:case 35670:return Mv;case 35667:case 35671:return Rv;case 35668:case 35672:return wv;case 35669:case 35673:return Cv;case 5125:return Nv;case 36294:return Iv;case 36295:return Bv;case 36296:return Fv;case 35678:case 36198:case 36298:case 36306:case 35682:return Lv;case 35679:case 36299:case 36307:return Uv;case 35680:case 36300:case 36308:case 36293:return Dv;case 36289:case 36303:case 36311:case 36292:return Pv}}function Vv(n,e){n.uniform1fv(this.addr,e)}function Ov(n,e){let t=Gr(e,this.size,2);n.uniform2fv(this.addr,t)}function kv(n,e){let t=Gr(e,this.size,3);n.uniform3fv(this.addr,t)}function zv(n,e){let t=Gr(e,this.size,4);n.uniform4fv(this.addr,t)}function Wv(n,e){let t=Gr(e,this.size,4);n.uniformMatrix2fv(this.addr,!1,t)}function Hv(n,e){let t=Gr(e,this.size,9);n.uniformMatrix3fv(this.addr,!1,t)}function Yv(n,e){let t=Gr(e,this.size,16);n.uniformMatrix4fv(this.addr,!1,t)}function Zv(n,e){n.uniform1iv(this.addr,e)}function Xv(n,e){n.uniform2iv(this.addr,e)}function Kv(n,e){n.uniform3iv(this.addr,e)}function $v(n,e){n.uniform4iv(this.addr,e)}function Jv(n,e){n.uniform1uiv(this.addr,e)}function qv(n,e){n.uniform2uiv(this.addr,e)}function Qv(n,e){n.uniform3uiv(this.addr,e)}function jv(n,e){n.uniform4uiv(this.addr,e)}function eA(n,e,t){let i=this.cache,s=e.length,r=_c(t,s);kt(i,r)||(n.uniform1iv(this.addr,r),zt(i,r));let o;this.type===n.SAMPLER_2D_SHADOW?o=Hh:o=Cm;for(let a=0;a!==s;++a)t.setTexture2D(e[a]||o,r[a])}function tA(n,e,t){let i=this.cache,s=e.length,r=_c(t,s);kt(i,r)||(n.uniform1iv(this.addr,r),zt(i,r));for(let o=0;o!==s;++o)t.setTexture3D(e[o]||Im,r[o])}function nA(n,e,t){let i=this.cache,s=e.length,r=_c(t,s);kt(i,r)||(n.uniform1iv(this.addr,r),zt(i,r));for(let o=0;o!==s;++o)t.setTextureCube(e[o]||Bm,r[o])}function iA(n,e,t){let i=this.cache,s=e.length,r=_c(t,s);kt(i,r)||(n.uniform1iv(this.addr,r),zt(i,r));for(let o=0;o!==s;++o)t.setTexture2DArray(e[o]||Nm,r[o])}function sA(n){switch(n){case 5126:return Vv;case 35664:return Ov;case 35665:return kv;case 35666:return zv;case 35674:return Wv;case 35675:return Hv;case 35676:return Yv;case 5124:case 35670:return Zv;case 35667:case 35671:return Xv;case 35668:case 35672:return Kv;case 35669:case 35673:return $v;case 5125:return Jv;case 36294:return qv;case 36295:return Qv;case 36296:return jv;case 35678:case 36198:case 36298:case 36306:case 35682:return eA;case 35679:case 36299:case 36307:return tA;case 35680:case 36300:case 36308:case 36293:return nA;case 36289:case 36303:case 36311:case 36292:return iA}}var Yh=class{constructor(e,t,i){this.id=e,this.addr=i,this.cache=[],this.type=t.type,this.setValue=Gv(t.type)}},Zh=class{constructor(e,t,i){this.id=e,this.addr=i,this.cache=[],this.type=t.type,this.size=t.size,this.setValue=sA(t.type)}},Xh=class{constructor(e){this.id=e,this.seq=[],this.map={}}setValue(e,t,i){let s=this.seq;for(let r=0,o=s.length;r!==o;++r){let a=s[r];a.setValue(e,t[a.id],i)}}},zh=/(\w+)(\])?(\[|\.)?/g;function pm(n,e){n.seq.push(e),n.map[e.id]=e}function rA(n,e,t){let i=n.name,s=i.length;for(zh.lastIndex=0;;){let r=zh.exec(i),o=zh.lastIndex,a=r[1],l=r[2]==="]",c=r[3];if(l&&(a=a|0),c===void 0||c==="["&&o+2===s){pm(t,c===void 0?new Yh(a,n,e):new Zh(a,n,e));break}else{let d=t.map[a];d===void 0&&(d=new Xh(a),pm(t,d)),t=d}}}var Pr=class{constructor(e,t){this.seq=[],this.map={};let i=e.getProgramParameter(t,e.ACTIVE_UNIFORMS);for(let o=0;o<i;++o){let a=e.getActiveUniform(t,o),l=e.getUniformLocation(t,a.name);rA(a,l,this)}let s=[],r=[];for(let o of this.seq)o.type===e.SAMPLER_2D_SHADOW||o.type===e.SAMPLER_CUBE_SHADOW||o.type===e.SAMPLER_2D_ARRAY_SHADOW?s.push(o):r.push(o);s.length>0&&(this.seq=s.concat(r))}setValue(e,t,i,s){let r=this.map[t];r!==void 0&&r.setValue(e,i,s)}setOptional(e,t,i){let s=t[i];s!==void 0&&this.setValue(e,i,s)}static upload(e,t,i,s){for(let r=0,o=t.length;r!==o;++r){let a=t[r],l=i[a.id];l.needsUpdate!==!1&&a.setValue(e,l.value,s)}}static seqWithValue(e,t){let i=[];for(let s=0,r=e.length;s!==r;++s){let o=e[s];o.id in t&&i.push(o)}return i}};function mm(n,e,t){let i=n.createShader(e);return n.shaderSource(i,t),n.compileShader(i),i}var oA=37297,aA=0;function lA(n,e){let t=n.split(`
`),i=[],s=Math.max(e-6,0),r=Math.min(e+6,t.length);for(let o=s;o<r;o++){let a=o+1;i.push(`${a===e?">":" "} ${a}: ${t[o]}`)}return i.join(`
`)}var gm=new Ge;function cA(n){Ve._getMatrix(gm,Ve.workingColorSpace,n);let e=`mat3( ${gm.elements.map(t=>t.toFixed(4))} )`;switch(Ve.getTransfer(n)){case cr:return[e,"LinearTransferOETF"];case nt:return[e,"sRGBTransferOETF"];default:return pe("WebGLProgram: Unsupported color space: ",n),[e,"LinearTransferOETF"]}}function xm(n,e,t){let i=n.getShaderParameter(e,n.COMPILE_STATUS),r=(n.getShaderInfoLog(e)||"").trim();if(i&&r==="")return"";let o=/ERROR: 0:(\d+)/.exec(r);if(o){let a=parseInt(o[1]);return t.toUpperCase()+`

`+r+`

`+lA(n.getShaderSource(e),a)}else return r}function uA(n,e){let t=cA(e);return[`vec4 ${n}( vec4 value ) {`,`	return ${t[1]}( vec4( value.rgb * ${t[0]}, value.a ) );`,"}"].join(`
`)}var hA={[Jl]:"Linear",[ql]:"Reinhard",[Ql]:"Cineon",[jl]:"ACESFilmic",[ec]:"AgX",[Tr]:"Neutral",[Th]:"Custom"};function dA(n,e){let t=hA[e];return t===void 0?(pe("WebGLProgram: Unsupported toneMapping:",e),"vec3 "+n+"( vec3 color ) { return LinearToneMapping( color ); }"):"vec3 "+n+"( vec3 color ) { return "+t+"ToneMapping( color ); }"}var pc=new L;function fA(){Ve.getLuminanceCoefficients(pc);let n=pc.x.toFixed(4),e=pc.y.toFixed(4),t=pc.z.toFixed(4);return["float luminance( const in vec3 rgb ) {",`	const vec3 weights = vec3( ${n}, ${e}, ${t} );`,"	return dot( weights, rgb );","}"].join(`
`)}function pA(n){return[n.extensionClipCullDistance?"#extension GL_ANGLE_clip_cull_distance : require":"",n.extensionMultiDraw?"#extension GL_ANGLE_multi_draw : require":""].filter(ya).join(`
`)}function mA(n){let e=[];for(let t in n){let i=n[t];i!==!1&&e.push("#define "+t+" "+i)}return e.join(`
`)}function gA(n,e){let t={},i=n.getProgramParameter(e,n.ACTIVE_ATTRIBUTES);for(let s=0;s<i;s++){let r=n.getActiveAttrib(e,s),o=r.name,a=1;r.type===n.FLOAT_MAT2&&(a=2),r.type===n.FLOAT_MAT3&&(a=3),r.type===n.FLOAT_MAT4&&(a=4),t[o]={type:r.type,location:n.getAttribLocation(e,o),locationSize:a}}return t}function ya(n){return n!==""}function ym(n,e){let t=e.numSpotLightShadows+e.numSpotLightMaps-e.numSpotLightShadowsWithMaps;return n.replace(/NUM_SUN_LIGHTS/g,e.numSunLights).replace(/NUM_DIR_LIGHTS/g,e.numDirLights).replace(/NUM_SPOT_LIGHTS/g,e.numSpotLights).replace(/NUM_SPOT_LIGHT_MAPS/g,e.numSpotLightMaps).replace(/NUM_SPOT_LIGHT_COORDS/g,t).replace(/NUM_RECT_AREA_LIGHTS/g,e.numRectAreaLights).replace(/NUM_POINT_LIGHTS/g,e.numPointLights).replace(/NUM_HEMI_LIGHTS/g,e.numHemiLights).replace(/NUM_SUN_LIGHT_SHADOWS/g,e.numSunLightShadows).replace(/NUM_DIR_LIGHT_SHADOWS/g,e.numDirLightShadows).replace(/NUM_SPOT_LIGHT_SHADOWS_WITH_MAPS/g,e.numSpotLightShadowsWithMaps).replace(/NUM_SPOT_LIGHT_SHADOWS/g,e.numSpotLightShadows).replace(/NUM_POINT_LIGHT_SHADOWS/g,e.numPointLightShadows)}function _m(n,e){return n.replace(/NUM_CLIPPING_PLANES/g,e.numClippingPlanes).replace(/UNION_CLIPPING_PLANES/g,e.numClippingPlanes-e.numClipIntersection)}var xA=/^[ \t]*#include +<([\w\d./]+)>/gm;function Kh(n){return n.replace(xA,_A)}var yA=new Map;function _A(n,e){let t=Xe[e];if(t===void 0){let i=yA.get(e);if(i!==void 0)t=Xe[i],pe('WebGLRenderer: Shader chunk "%s" has been deprecated. Use "%s" instead.',e,i);else throw new Error("THREE.WebGLProgram: Can not resolve #include <"+e+">")}return Kh(t)}var bA=/#pragma unroll_loop_start\s+for\s*\(\s*int\s+i\s*=\s*(\d+)\s*;\s*i\s*<\s*(\d+)\s*;\s*i\s*\+\+\s*\)\s*{([\s\S]+?)}\s+#pragma unroll_loop_end/g;function bm(n){return n.replace(bA,SA)}function SA(n,e,t,i){let s="";for(let r=parseInt(e);r<parseInt(t);r++)s+=i.replace(/\[\s*i\s*\]/g,"[ "+r+" ]").replace(/UNROLLED_LOOP_INDEX/g,r);return s}function Sm(n){let e=`precision ${n.precision} float;
	precision ${n.precision} int;
	precision ${n.precision} sampler2D;
	precision ${n.precision} samplerCube;
	precision ${n.precision} sampler3D;
	precision ${n.precision} sampler2DArray;
	precision ${n.precision} sampler2DShadow;
	precision ${n.precision} samplerCubeShadow;
	precision ${n.precision} sampler2DArrayShadow;
	precision ${n.precision} isampler2D;
	precision ${n.precision} isampler3D;
	precision ${n.precision} isamplerCube;
	precision ${n.precision} isampler2DArray;
	precision ${n.precision} usampler2D;
	precision ${n.precision} usampler3D;
	precision ${n.precision} usamplerCube;
	precision ${n.precision} usampler2DArray;
	`;return n.precision==="highp"?e+=`
#define HIGH_PRECISION`:n.precision==="mediump"?e+=`
#define MEDIUM_PRECISION`:n.precision==="lowp"&&(e+=`
#define LOW_PRECISION`),e}var vA={[Ar]:"SHADOWMAP_TYPE_PCF",[bs]:"SHADOWMAP_TYPE_VSM"};function AA(n){return vA[n.shadowMapType]||"SHADOWMAP_TYPE_BASIC"}var TA={[hi]:"ENVMAP_TYPE_CUBE",[Bi]:"ENVMAP_TYPE_CUBE",[Er]:"ENVMAP_TYPE_CUBE_UV"};function EA(n){return n.envMap===!1?"ENVMAP_TYPE_CUBE":TA[n.envMapMode]||"ENVMAP_TYPE_CUBE"}var MA={[Bi]:"ENVMAP_MODE_REFRACTION"};function RA(n){return n.envMap===!1?"ENVMAP_MODE_REFLECTION":MA[n.envMapMode]||"ENVMAP_MODE_REFLECTION"}var wA={[$l]:"ENVMAP_BLENDING_MULTIPLY",[vh]:"ENVMAP_BLENDING_MIX",[Ah]:"ENVMAP_BLENDING_ADD"};function CA(n){return n.envMap===!1?"ENVMAP_BLENDING_NONE":wA[n.combine]||"ENVMAP_BLENDING_NONE"}function NA(n){let e=n.envMapCubeUVHeight;if(e===null)return null;let t=Math.log2(e)-2,i=1/e;return{texelWidth:1/(3*Math.max(Math.pow(2,t),112)),texelHeight:i,maxMip:t}}function IA(n,e,t,i){let s=n.getContext(),r=t.defines,o=t.vertexShader,a=t.fragmentShader,l=AA(t),c=EA(t),u=RA(t),d=CA(t),h=NA(t),f=pA(t),g=mA(r),y=s.createProgram(),m,p,E=t.glslVersion?"#version "+t.glslVersion+`
`:"";t.isRawShaderMaterial?(m=["#define SHADER_TYPE "+t.shaderType,"#define SHADER_NAME "+t.shaderName,g].filter(ya).join(`
`),m.length>0&&(m+=`
`),p=["#define SHADER_TYPE "+t.shaderType,"#define SHADER_NAME "+t.shaderName,g].filter(ya).join(`
`),p.length>0&&(p+=`
`)):(m=[Sm(t),"#define SHADER_TYPE "+t.shaderType,"#define SHADER_NAME "+t.shaderName,g,t.extensionClipCullDistance?"#define USE_CLIP_DISTANCE":"",t.batching?"#define USE_BATCHING":"",t.batchingColor?"#define USE_BATCHING_COLOR":"",t.instancing?"#define USE_INSTANCING":"",t.instancingColor?"#define USE_INSTANCING_COLOR":"",t.instancingMorph?"#define USE_INSTANCING_MORPH":"",t.useFog&&t.fog?"#define USE_FOG":"",t.useFog&&t.fogExp2?"#define FOG_EXP2":"",t.map?"#define USE_MAP":"",t.envMap?"#define USE_ENVMAP":"",t.envMap?"#define "+u:"",t.lightMap?"#define USE_LIGHTMAP":"",t.aoMap?"#define USE_AOMAP":"",t.bumpMap?"#define USE_BUMPMAP":"",t.normalMap?"#define USE_NORMALMAP":"",t.normalMapObjectSpace?"#define USE_NORMALMAP_OBJECTSPACE":"",t.normalMapTangentSpace?"#define USE_NORMALMAP_TANGENTSPACE":"",t.displacementMap?"#define USE_DISPLACEMENTMAP":"",t.emissiveMap?"#define USE_EMISSIVEMAP":"",t.anisotropy?"#define USE_ANISOTROPY":"",t.anisotropyMap?"#define USE_ANISOTROPYMAP":"",t.clearcoatMap?"#define USE_CLEARCOATMAP":"",t.clearcoatRoughnessMap?"#define USE_CLEARCOAT_ROUGHNESSMAP":"",t.clearcoatNormalMap?"#define USE_CLEARCOAT_NORMALMAP":"",t.iridescenceMap?"#define USE_IRIDESCENCEMAP":"",t.iridescenceThicknessMap?"#define USE_IRIDESCENCE_THICKNESSMAP":"",t.specularMap?"#define USE_SPECULARMAP":"",t.specularColorMap?"#define USE_SPECULAR_COLORMAP":"",t.specularIntensityMap?"#define USE_SPECULAR_INTENSITYMAP":"",t.roughnessMap?"#define USE_ROUGHNESSMAP":"",t.metalnessMap?"#define USE_METALNESSMAP":"",t.alphaMap?"#define USE_ALPHAMAP":"",t.alphaHash?"#define USE_ALPHAHASH":"",t.transmission?"#define USE_TRANSMISSION":"",t.transmissionMap?"#define USE_TRANSMISSIONMAP":"",t.thicknessMap?"#define USE_THICKNESSMAP":"",t.sheenColorMap?"#define USE_SHEEN_COLORMAP":"",t.sheenRoughnessMap?"#define USE_SHEEN_ROUGHNESSMAP":"",t.mapUv?"#define MAP_UV "+t.mapUv:"",t.alphaMapUv?"#define ALPHAMAP_UV "+t.alphaMapUv:"",t.lightMapUv?"#define LIGHTMAP_UV "+t.lightMapUv:"",t.aoMapUv?"#define AOMAP_UV "+t.aoMapUv:"",t.emissiveMapUv?"#define EMISSIVEMAP_UV "+t.emissiveMapUv:"",t.bumpMapUv?"#define BUMPMAP_UV "+t.bumpMapUv:"",t.normalMapUv?"#define NORMALMAP_UV "+t.normalMapUv:"",t.displacementMapUv?"#define DISPLACEMENTMAP_UV "+t.displacementMapUv:"",t.metalnessMapUv?"#define METALNESSMAP_UV "+t.metalnessMapUv:"",t.roughnessMapUv?"#define ROUGHNESSMAP_UV "+t.roughnessMapUv:"",t.anisotropyMapUv?"#define ANISOTROPYMAP_UV "+t.anisotropyMapUv:"",t.clearcoatMapUv?"#define CLEARCOATMAP_UV "+t.clearcoatMapUv:"",t.clearcoatNormalMapUv?"#define CLEARCOAT_NORMALMAP_UV "+t.clearcoatNormalMapUv:"",t.clearcoatRoughnessMapUv?"#define CLEARCOAT_ROUGHNESSMAP_UV "+t.clearcoatRoughnessMapUv:"",t.iridescenceMapUv?"#define IRIDESCENCEMAP_UV "+t.iridescenceMapUv:"",t.iridescenceThicknessMapUv?"#define IRIDESCENCE_THICKNESSMAP_UV "+t.iridescenceThicknessMapUv:"",t.sheenColorMapUv?"#define SHEEN_COLORMAP_UV "+t.sheenColorMapUv:"",t.sheenRoughnessMapUv?"#define SHEEN_ROUGHNESSMAP_UV "+t.sheenRoughnessMapUv:"",t.specularMapUv?"#define SPECULARMAP_UV "+t.specularMapUv:"",t.specularColorMapUv?"#define SPECULAR_COLORMAP_UV "+t.specularColorMapUv:"",t.specularIntensityMapUv?"#define SPECULAR_INTENSITYMAP_UV "+t.specularIntensityMapUv:"",t.transmissionMapUv?"#define TRANSMISSIONMAP_UV "+t.transmissionMapUv:"",t.thicknessMapUv?"#define THICKNESSMAP_UV "+t.thicknessMapUv:"",t.vertexTangents&&t.flatShading===!1?"#define USE_TANGENT":"",t.vertexNormals?"#define HAS_NORMAL":"",t.vertexColors?"#define USE_COLOR":"",t.vertexAlphas?"#define USE_COLOR_ALPHA":"",t.vertexUv1s?"#define USE_UV1":"",t.vertexUv2s?"#define USE_UV2":"",t.vertexUv3s?"#define USE_UV3":"",t.pointsUvs?"#define USE_POINTS_UV":"",t.flatShading?"#define FLAT_SHADED":"",t.skinning?"#define USE_SKINNING":"",t.morphTargets?"#define USE_MORPHTARGETS":"",t.morphNormals&&t.flatShading===!1?"#define USE_MORPHNORMALS":"",t.morphColors?"#define USE_MORPHCOLORS":"",t.morphTargetsCount>0?"#define MORPHTARGETS_TEXTURE_STRIDE "+t.morphTextureStride:"",t.morphTargetsCount>0?"#define MORPHTARGETS_COUNT "+t.morphTargetsCount:"",t.doubleSided?"#define DOUBLE_SIDED":"",t.flipSided?"#define FLIP_SIDED":"",t.shadowMapEnabled?"#define USE_SHADOWMAP":"",t.shadowMapEnabled?"#define "+l:"",t.sizeAttenuation?"#define USE_SIZEATTENUATION":"",t.numLightProbes>0?"#define USE_LIGHT_PROBES":"",t.logarithmicDepthBuffer?"#define USE_LOGARITHMIC_DEPTH_BUFFER":"",t.reversedDepthBuffer?"#define USE_REVERSED_DEPTH_BUFFER":"","uniform mat4 modelMatrix;","uniform mat4 modelViewMatrix;","uniform mat4 projectionMatrix;","uniform mat4 viewMatrix;","uniform mat3 normalMatrix;","uniform vec3 cameraPosition;","uniform bool isOrthographic;","#ifdef USE_INSTANCING","	attribute mat4 instanceMatrix;","#endif","#ifdef USE_INSTANCING_COLOR","	attribute vec3 instanceColor;","#endif","#ifdef USE_INSTANCING_MORPH","	uniform sampler2D morphTexture;","#endif","attribute vec3 position;","attribute vec3 normal;","attribute vec2 uv;","#ifdef USE_UV1","	attribute vec2 uv1;","#endif","#ifdef USE_UV2","	attribute vec2 uv2;","#endif","#ifdef USE_UV3","	attribute vec2 uv3;","#endif","#ifdef USE_TANGENT","	attribute vec4 tangent;","#endif","#if defined( USE_COLOR_ALPHA )","	attribute vec4 color;","#elif defined( USE_COLOR )","	attribute vec3 color;","#endif","#ifdef USE_SKINNING","	attribute vec4 skinIndex;","	attribute vec4 skinWeight;","#endif",`
`].filter(ya).join(`
`),p=[Sm(t),"#define SHADER_TYPE "+t.shaderType,"#define SHADER_NAME "+t.shaderName,g,t.useFog&&t.fog?"#define USE_FOG":"",t.useFog&&t.fogExp2?"#define FOG_EXP2":"",t.alphaToCoverage?"#define ALPHA_TO_COVERAGE":"",t.map?"#define USE_MAP":"",t.matcap?"#define USE_MATCAP":"",t.envMap?"#define USE_ENVMAP":"",t.envMap?"#define "+c:"",t.envMap?"#define "+u:"",t.envMap?"#define "+d:"",h?"#define CUBEUV_TEXEL_WIDTH "+h.texelWidth:"",h?"#define CUBEUV_TEXEL_HEIGHT "+h.texelHeight:"",h?"#define CUBEUV_MAX_MIP "+h.maxMip+".0":"",t.lightMap?"#define USE_LIGHTMAP":"",t.aoMap?"#define USE_AOMAP":"",t.bumpMap?"#define USE_BUMPMAP":"",t.normalMap?"#define USE_NORMALMAP":"",t.normalMapObjectSpace?"#define USE_NORMALMAP_OBJECTSPACE":"",t.normalMapTangentSpace?"#define USE_NORMALMAP_TANGENTSPACE":"",t.packedNormalMap?"#define USE_PACKED_NORMALMAP":"",t.emissiveMap?"#define USE_EMISSIVEMAP":"",t.anisotropy?"#define USE_ANISOTROPY":"",t.anisotropyMap?"#define USE_ANISOTROPYMAP":"",t.clearcoat?"#define USE_CLEARCOAT":"",t.clearcoatMap?"#define USE_CLEARCOATMAP":"",t.clearcoatRoughnessMap?"#define USE_CLEARCOAT_ROUGHNESSMAP":"",t.clearcoatNormalMap?"#define USE_CLEARCOAT_NORMALMAP":"",t.dispersion?"#define USE_DISPERSION":"",t.retroreflection?"#define USE_RETROREFLECTION":"",t.iridescence?"#define USE_IRIDESCENCE":"",t.iridescenceMap?"#define USE_IRIDESCENCEMAP":"",t.iridescenceThicknessMap?"#define USE_IRIDESCENCE_THICKNESSMAP":"",t.specularMap?"#define USE_SPECULARMAP":"",t.specularColorMap?"#define USE_SPECULAR_COLORMAP":"",t.specularIntensityMap?"#define USE_SPECULAR_INTENSITYMAP":"",t.roughnessMap?"#define USE_ROUGHNESSMAP":"",t.metalnessMap?"#define USE_METALNESSMAP":"",t.alphaMap?"#define USE_ALPHAMAP":"",t.alphaTest?"#define USE_ALPHATEST":"",t.alphaHash?"#define USE_ALPHAHASH":"",t.sheen?"#define USE_SHEEN":"",t.sheenColorMap?"#define USE_SHEEN_COLORMAP":"",t.sheenRoughnessMap?"#define USE_SHEEN_ROUGHNESSMAP":"",t.transmission?"#define USE_TRANSMISSION":"",t.transmissionMap?"#define USE_TRANSMISSIONMAP":"",t.thicknessMap?"#define USE_THICKNESSMAP":"",t.vertexTangents&&t.flatShading===!1?"#define USE_TANGENT":"",t.vertexColors||t.instancingColor?"#define USE_COLOR":"",t.vertexAlphas||t.batchingColor?"#define USE_COLOR_ALPHA":"",t.vertexUv1s?"#define USE_UV1":"",t.vertexUv2s?"#define USE_UV2":"",t.vertexUv3s?"#define USE_UV3":"",t.pointsUvs?"#define USE_POINTS_UV":"",t.gradientMap?"#define USE_GRADIENTMAP":"",t.flatShading?"#define FLAT_SHADED":"",t.doubleSided?"#define DOUBLE_SIDED":"",t.flipSided?"#define FLIP_SIDED":"",t.shadowMapEnabled?"#define USE_SHADOWMAP":"",t.shadowMapEnabled?"#define "+l:"",t.premultipliedAlpha?"#define PREMULTIPLIED_ALPHA":"",t.numLightProbes>0?"#define USE_LIGHT_PROBES":"",t.numLightProbeGrids>0?"#define USE_LIGHT_PROBES_GRID":"",t.decodeVideoTexture?"#define DECODE_VIDEO_TEXTURE":"",t.decodeVideoTextureEmissive?"#define DECODE_VIDEO_TEXTURE_EMISSIVE":"",t.logarithmicDepthBuffer?"#define USE_LOGARITHMIC_DEPTH_BUFFER":"",t.reversedDepthBuffer?"#define USE_REVERSED_DEPTH_BUFFER":"","uniform mat4 viewMatrix;","uniform vec3 cameraPosition;","uniform bool isOrthographic;",t.toneMapping!==pn?"#define TONE_MAPPING":"",t.toneMapping!==pn?Xe.tonemapping_pars_fragment:"",t.toneMapping!==pn?dA("toneMapping",t.toneMapping):"",t.dithering?"#define DITHERING":"",t.opaque?"#define OPAQUE":"",Xe.colorspace_pars_fragment,uA("linearToOutputTexel",t.outputColorSpace),fA(),t.useDepthPacking?"#define DEPTH_PACKING "+t.depthPacking:"",`
`].filter(ya).join(`
`)),o=Kh(o),o=ym(o,t),o=_m(o,t),a=Kh(a),a=ym(a,t),a=_m(a,t),o=bm(o),a=bm(a),t.isRawShaderMaterial!==!0&&(E=`#version 300 es
`,m=[f,"#define attribute in","#define varying out","#define texture2D texture"].join(`
`)+`
`+m,p=["#define varying in",t.glslVersion===Ch?"":"layout(location = 0) out highp vec4 pc_fragColor;",t.glslVersion===Ch?"":"#define gl_FragColor pc_fragColor","#define gl_FragDepthEXT gl_FragDepth","#define texture2D texture","#define textureCube texture","#define texture2DProj textureProj","#define texture2DLodEXT textureLod","#define texture2DProjLodEXT textureProjLod","#define textureCubeLodEXT textureLod","#define texture2DGradEXT textureGrad","#define texture2DProjGradEXT textureProjGrad","#define textureCubeGradEXT textureGrad"].join(`
`)+`
`+p);let w=E+m+o,S=E+p+a,v=mm(s,s.VERTEX_SHADER,w),A=mm(s,s.FRAGMENT_SHADER,S);s.attachShader(y,v),s.attachShader(y,A),t.index0AttributeName!==void 0?s.bindAttribLocation(y,0,t.index0AttributeName):t.hasPositionAttribute===!0&&s.bindAttribLocation(y,0,"position"),s.linkProgram(y);function R(D){if(n.debug.checkShaderErrors){let k=s.getProgramInfoLog(y)||"",H=s.getShaderInfoLog(v)||"",F=s.getShaderInfoLog(A)||"",O=k.trim(),q=H.trim(),X=F.trim(),re=!0,K=!0;if(s.getProgramParameter(y,s.LINK_STATUS)===!1)if(re=!1,typeof n.debug.onShaderError=="function")n.debug.onShaderError(s,y,v,A);else{let ne=xm(s,v,"vertex"),se=xm(s,A,"fragment");_e("WebGLProgram: Shader Error "+s.getError()+" - VALIDATE_STATUS "+s.getProgramParameter(y,s.VALIDATE_STATUS)+`

Material Name: `+D.name+`
Material Type: `+D.type+`

Program Info Log: `+O+`
`+ne+`
`+se)}else O!==""?pe("WebGLProgram: Program Info Log:",O):(q===""||X==="")&&(K=!1);K&&(D.diagnostics={runnable:re,programLog:O,vertexShader:{log:q,prefix:m},fragmentShader:{log:X,prefix:p}})}s.deleteShader(v),s.deleteShader(A),b=new Pr(s,y),M=gA(s,y)}let b;this.getUniforms=function(){return b===void 0&&R(this),b};let M;this.getAttributes=function(){return M===void 0&&R(this),M};let B=t.rendererExtensionParallelShaderCompile===!1;return this.isReady=function(){return B===!1&&(B=s.getProgramParameter(y,oA)),B},this.destroy=function(){i.releaseStatesOfProgram(this),s.deleteProgram(y),this.program=void 0},this.type=t.shaderType,this.name=t.shaderName,this.id=aA++,this.cacheKey=e,this.usedTimes=1,this.program=y,this.vertexShader=v,this.fragmentShader=A,this}var BA=0,$h=class{constructor(){this.shaderCache=new Map,this.materialCache=new Map}update(e,t,i){let s=this._getShaderCacheForMaterial(e);return s.has(t)===!1&&(s.add(t),t.usedTimes++),s.has(i)===!1&&(s.add(i),i.usedTimes++),this}remove(e){let t=this.materialCache.get(e);for(let i of t)i.usedTimes--,i.usedTimes===0&&this.shaderCache.delete(i.code);return this.materialCache.delete(e),this}getVertexShaderStage(e){return this._getShaderStage(e.vertexShader)}getFragmentShaderStage(e){return this._getShaderStage(e.fragmentShader)}dispose(){this.shaderCache.clear(),this.materialCache.clear()}_getShaderCacheForMaterial(e){let t=this.materialCache,i=t.get(e);return i===void 0&&(i=new Set,t.set(e,i)),i}_getShaderStage(e){let t=this.shaderCache,i=t.get(e);return i===void 0&&(i=new Jh(e),t.set(e,i)),i}},Jh=class{constructor(e){this.id=BA++,this.code=e,this.usedTimes=0}};function FA(n){return n===Qn||n===Ms||n===Rs}function LA(n,e,t,i,s,r){let o=new pr,a=new $h,l=new Set,c=[],u=new Map,d=i.logarithmicDepthBuffer,h=i.precision,f={MeshDepthMaterial:"depth",MeshDistanceMaterial:"distance",MeshNormalMaterial:"normal",MeshBasicMaterial:"basic",MeshLambertMaterial:"lambert",MeshPhongMaterial:"phong",MeshToonMaterial:"toon",MeshStandardMaterial:"physical",MeshPhysicalMaterial:"physical",MeshMatcapMaterial:"matcap",LineBasicMaterial:"basic",LineDashedMaterial:"dashed",PointsMaterial:"points",ShadowMaterial:"shadow",SpriteMaterial:"sprite"};function g(b){return l.add(b),b===0?"uv":`uv${b}`}function y(b,M,B,D,k,H){let F=D.fog,O=k.geometry,q=b.isMeshStandardMaterial||b.isMeshLambertMaterial||b.isMeshPhongMaterial?D.environment:null,X=b.isMeshStandardMaterial||b.isMeshLambertMaterial&&!b.envMap||b.isMeshPhongMaterial&&!b.envMap,re=e.get(b.envMap||q,X),K=re&&re.mapping===Er?re.image.height:null,ne=f[b.type];b.precision!==null&&(h=i.getMaxPrecision(b.precision),h!==b.precision&&pe("WebGLProgram.getParameters:",b.precision,"not supported, using",h,"instead."));let se=O.morphAttributes.position||O.morphAttributes.normal||O.morphAttributes.color,Ue=se!==void 0?se.length:0,Fe=0;O.morphAttributes.position!==void 0&&(Fe=1),O.morphAttributes.normal!==void 0&&(Fe=2),O.morphAttributes.color!==void 0&&(Fe=3);let yt,je,rt,$;if(ne){let bt=mi[ne];yt=bt.vertexShader,je=bt.fragmentShader}else{yt=b.vertexShader,je=b.fragmentShader;let bt=a.getVertexShaderStage(b),at=a.getFragmentShaderStage(b);a.update(b,bt,at),rt=bt.id,$=at.id}let te=n.getRenderTarget(),Te=n.state.buffers.depth.getReversed(),Oe=k.isInstancedMesh===!0,ve=k.isBatchedMesh===!0,Ke=!!b.map,Ot=!!b.matcap,$e=!!re,st=!!b.aoMap,_t=!!b.lightMap,Qe=!!b.bumpMap&&b.wireframe===!1,Tt=!!b.normalMap,Yt=!!b.displacementMap,xn=!!b.emissiveMap,Mt=!!b.metalnessMap,Ft=!!b.roughnessMap,I=b.anisotropy>0,Qt=b.clearcoat>0,ut=b.dispersion>0,T=b.retroreflectivity>0,x=b.iridescence>0,U=b.sheen>0,z=b.transmission>0,Z=I&&!!b.anisotropyMap,ae=Qt&&!!b.clearcoatMap,le=Qt&&!!b.clearcoatNormalMap,J=Qt&&!!b.clearcoatRoughnessMap,j=x&&!!b.iridescenceMap,ce=x&&!!b.iridescenceThicknessMap,Ie=U&&!!b.sheenColorMap,fe=U&&!!b.sheenRoughnessMap,ue=!!b.specularMap,Be=!!b.specularColorMap,Pe=!!b.specularIntensityMap,We=z&&!!b.transmissionMap,N=z&&!!b.thicknessMap,he=!!b.gradientMap,Q=!!b.alphaMap,de=b.alphaTest>0,be=!!b.alphaHash,ie=!!b.extensions,Le=pn;b.toneMapped&&(te===null||te.isXRRenderTarget===!0)&&(Le=n.toneMapping);let Re={shaderID:ne,shaderType:b.type,shaderName:b.name,vertexShader:yt,fragmentShader:je,defines:b.defines,customVertexShaderID:rt,customFragmentShaderID:$,isRawShaderMaterial:b.isRawShaderMaterial===!0,glslVersion:b.glslVersion,precision:h,batching:ve,batchingColor:ve&&k._colorsTexture!==null,instancing:Oe,instancingColor:Oe&&k.instanceColor!==null,instancingMorph:Oe&&k.morphTexture!==null,outputColorSpace:te===null?n.outputColorSpace:te.isXRRenderTarget===!0?te.texture.colorSpace:Ve.workingColorSpace,alphaToCoverage:!!b.alphaToCoverage,map:Ke,matcap:Ot,envMap:$e,envMapMode:$e&&re.mapping,envMapCubeUVHeight:K,aoMap:st,lightMap:_t,bumpMap:Qe,normalMap:Tt,displacementMap:Yt,emissiveMap:xn,normalMapObjectSpace:Tt&&b.normalMapType===oc,normalMapTangentSpace:Tt&&b.normalMapType===Ki,packedNormalMap:Tt&&b.normalMapType===Ki&&FA(b.normalMap.format),metalnessMap:Mt,roughnessMap:Ft,anisotropy:I,anisotropyMap:Z,clearcoat:Qt,clearcoatMap:ae,clearcoatNormalMap:le,clearcoatRoughnessMap:J,dispersion:ut,retroreflection:T,iridescence:x,iridescenceMap:j,iridescenceThicknessMap:ce,sheen:U,sheenColorMap:Ie,sheenRoughnessMap:fe,specularMap:ue,specularColorMap:Be,specularIntensityMap:Pe,transmission:z,transmissionMap:We,thicknessMap:N,gradientMap:he,opaque:b.transparent===!1&&b.blending===Ss&&b.alphaToCoverage===!1,alphaMap:Q,alphaTest:de,alphaHash:be,combine:b.combine,mapUv:Ke&&g(b.map.channel),aoMapUv:st&&g(b.aoMap.channel),lightMapUv:_t&&g(b.lightMap.channel),bumpMapUv:Qe&&g(b.bumpMap.channel),normalMapUv:Tt&&g(b.normalMap.channel),displacementMapUv:Yt&&g(b.displacementMap.channel),emissiveMapUv:xn&&g(b.emissiveMap.channel),metalnessMapUv:Mt&&g(b.metalnessMap.channel),roughnessMapUv:Ft&&g(b.roughnessMap.channel),anisotropyMapUv:Z&&g(b.anisotropyMap.channel),clearcoatMapUv:ae&&g(b.clearcoatMap.channel),clearcoatNormalMapUv:le&&g(b.clearcoatNormalMap.channel),clearcoatRoughnessMapUv:J&&g(b.clearcoatRoughnessMap.channel),iridescenceMapUv:j&&g(b.iridescenceMap.channel),iridescenceThicknessMapUv:ce&&g(b.iridescenceThicknessMap.channel),sheenColorMapUv:Ie&&g(b.sheenColorMap.channel),sheenRoughnessMapUv:fe&&g(b.sheenRoughnessMap.channel),specularMapUv:ue&&g(b.specularMap.channel),specularColorMapUv:Be&&g(b.specularColorMap.channel),specularIntensityMapUv:Pe&&g(b.specularIntensityMap.channel),transmissionMapUv:We&&g(b.transmissionMap.channel),thicknessMapUv:N&&g(b.thicknessMap.channel),alphaMapUv:Q&&g(b.alphaMap.channel),vertexTangents:!!O.attributes.tangent&&(Tt||I),vertexNormals:!!O.attributes.normal,vertexColors:b.vertexColors,vertexAlphas:b.vertexColors===!0&&!!O.attributes.color&&O.attributes.color.itemSize===4,pointsUvs:k.isPoints===!0&&!!O.attributes.uv&&(Ke||Q),fog:!!F,useFog:b.fog===!0,fogExp2:!!F&&F.isFogExp2,flatShading:b.wireframe===!1&&(b.flatShading===!0||O.attributes.normal===void 0&&Tt===!1&&(b.isMeshLambertMaterial||b.isMeshPhongMaterial||b.isMeshStandardMaterial||b.isMeshPhysicalMaterial)),sizeAttenuation:b.sizeAttenuation===!0,logarithmicDepthBuffer:d,reversedDepthBuffer:Te,skinning:k.isSkinnedMesh===!0,hasPositionAttribute:O.attributes.position!==void 0,morphTargets:O.morphAttributes.position!==void 0,morphNormals:O.morphAttributes.normal!==void 0,morphColors:O.morphAttributes.color!==void 0,morphTargetsCount:Ue,morphTextureStride:Fe,numSunLights:M.sun.length,numDirLights:M.directional.length,numPointLights:M.point.length,numSpotLights:M.spot.length,numSpotLightMaps:M.spotLightMap.length,numRectAreaLights:M.rectArea.length,numHemiLights:M.hemi.length,numSunLightShadows:M.sunShadowMap.length,numDirLightShadows:M.directionalShadowMap.length,numPointLightShadows:M.pointShadowMap.length,numSpotLightShadows:M.spotShadowMap.length,numSpotLightShadowsWithMaps:M.numSpotLightShadowsWithMaps,numLightProbes:M.numLightProbes,numLightProbeGrids:H.length,numClippingPlanes:r.numPlanes,numClipIntersection:r.numIntersection,dithering:b.dithering,shadowMapEnabled:n.shadowMap.enabled&&B.length>0,shadowMapType:n.shadowMap.type,toneMapping:Le,decodeVideoTexture:Ke&&b.map.isVideoTexture===!0&&Ve.getTransfer(b.map.colorSpace)===nt,decodeVideoTextureEmissive:xn&&b.emissiveMap.isVideoTexture===!0&&Ve.getTransfer(b.emissiveMap.colorSpace)===nt,premultipliedAlpha:b.premultipliedAlpha,doubleSided:b.side===sn,flipSided:b.side===Dt,useDepthPacking:b.depthPacking>=0,depthPacking:b.depthPacking||0,index0AttributeName:b.index0AttributeName,extensionClipCullDistance:ie&&b.extensions.clipCullDistance===!0&&t.has("WEBGL_clip_cull_distance"),extensionMultiDraw:(ie&&b.extensions.multiDraw===!0||ve)&&t.has("WEBGL_multi_draw"),rendererExtensionParallelShaderCompile:t.has("KHR_parallel_shader_compile"),customProgramCacheKey:b.customProgramCacheKey()};return Re.vertexUv1s=l.has(1),Re.vertexUv2s=l.has(2),Re.vertexUv3s=l.has(3),l.clear(),Re}function m(b){let M=[];if(b.shaderID?M.push(b.shaderID):(M.push(b.customVertexShaderID),M.push(b.customFragmentShaderID)),b.defines!==void 0)for(let B in b.defines)M.push(B),M.push(b.defines[B]);return b.isRawShaderMaterial===!1&&(p(M,b),E(M,b),M.push(n.outputColorSpace)),M.push(b.customProgramCacheKey),M.join()}function p(b,M){b.push(M.precision),b.push(M.outputColorSpace),b.push(M.envMapMode),b.push(M.envMapCubeUVHeight),b.push(M.mapUv),b.push(M.alphaMapUv),b.push(M.lightMapUv),b.push(M.aoMapUv),b.push(M.bumpMapUv),b.push(M.normalMapUv),b.push(M.displacementMapUv),b.push(M.emissiveMapUv),b.push(M.metalnessMapUv),b.push(M.roughnessMapUv),b.push(M.anisotropyMapUv),b.push(M.clearcoatMapUv),b.push(M.clearcoatNormalMapUv),b.push(M.clearcoatRoughnessMapUv),b.push(M.iridescenceMapUv),b.push(M.iridescenceThicknessMapUv),b.push(M.sheenColorMapUv),b.push(M.sheenRoughnessMapUv),b.push(M.specularMapUv),b.push(M.specularColorMapUv),b.push(M.specularIntensityMapUv),b.push(M.transmissionMapUv),b.push(M.thicknessMapUv),b.push(M.combine),b.push(M.fogExp2),b.push(M.sizeAttenuation),b.push(M.morphTargetsCount),b.push(M.morphAttributeCount),b.push(M.numSunLights),b.push(M.numDirLights),b.push(M.numPointLights),b.push(M.numSpotLights),b.push(M.numSpotLightMaps),b.push(M.numHemiLights),b.push(M.numRectAreaLights),b.push(M.numSunLightShadows),b.push(M.numDirLightShadows),b.push(M.numPointLightShadows),b.push(M.numSpotLightShadows),b.push(M.numSpotLightShadowsWithMaps),b.push(M.numLightProbes),b.push(M.shadowMapType),b.push(M.toneMapping),b.push(M.numClippingPlanes),b.push(M.numClipIntersection),b.push(M.depthPacking)}function E(b,M){o.disableAll(),M.instancing&&o.enable(0),M.instancingColor&&o.enable(1),M.instancingMorph&&o.enable(2),M.matcap&&o.enable(3),M.envMap&&o.enable(4),M.normalMapObjectSpace&&o.enable(5),M.normalMapTangentSpace&&o.enable(6),M.clearcoat&&o.enable(7),M.iridescence&&o.enable(8),M.alphaTest&&o.enable(9),M.vertexColors&&o.enable(10),M.vertexAlphas&&o.enable(11),M.vertexUv1s&&o.enable(12),M.vertexUv2s&&o.enable(13),M.vertexUv3s&&o.enable(14),M.vertexTangents&&o.enable(15),M.anisotropy&&o.enable(16),M.alphaHash&&o.enable(17),M.batching&&o.enable(18),M.dispersion&&o.enable(19),M.retroreflection&&o.enable(24),M.batchingColor&&o.enable(20),M.gradientMap&&o.enable(21),M.packedNormalMap&&o.enable(22),M.vertexNormals&&o.enable(23),b.push(o.mask),o.disableAll(),M.fog&&o.enable(0),M.useFog&&o.enable(1),M.flatShading&&o.enable(2),M.logarithmicDepthBuffer&&o.enable(3),M.reversedDepthBuffer&&o.enable(4),M.skinning&&o.enable(5),M.morphTargets&&o.enable(6),M.morphNormals&&o.enable(7),M.morphColors&&o.enable(8),M.premultipliedAlpha&&o.enable(9),M.shadowMapEnabled&&o.enable(10),M.doubleSided&&o.enable(11),M.flipSided&&o.enable(12),M.useDepthPacking&&o.enable(13),M.dithering&&o.enable(14),M.transmission&&o.enable(15),M.sheen&&o.enable(16),M.opaque&&o.enable(17),M.pointsUvs&&o.enable(18),M.decodeVideoTexture&&o.enable(19),M.decodeVideoTextureEmissive&&o.enable(20),M.alphaToCoverage&&o.enable(21),M.numLightProbeGrids>0&&o.enable(22),M.hasPositionAttribute&&o.enable(23),b.push(o.mask)}function w(b){let M=f[b.type],B;if(M){let D=mi[M];B=nm.clone(D.uniforms)}else B=b.uniforms;return B}function S(b,M){let B=u.get(M);return B!==void 0?++B.usedTimes:(B=new IA(n,M,b,s),c.push(B),u.set(M,B)),B}function v(b){if(--b.usedTimes===0){let M=c.indexOf(b);c[M]=c[c.length-1],c.pop(),u.delete(b.cacheKey),b.destroy()}}function A(b){a.remove(b)}function R(){a.dispose()}return{getParameters:y,getProgramCacheKey:m,getUniforms:w,acquireProgram:S,releaseProgram:v,releaseShaderCache:A,programs:c,dispose:R}}function UA(){let n=new WeakMap;function e(o){return n.has(o)}function t(o){let a=n.get(o);return a===void 0&&(a={},n.set(o,a)),a}function i(o){n.delete(o)}function s(o,a,l){n.get(o)[a]=l}function r(){n=new WeakMap}return{has:e,get:t,remove:i,update:s,dispose:r}}function DA(n,e){return n.groupOrder!==e.groupOrder?n.groupOrder-e.groupOrder:n.renderOrder!==e.renderOrder?n.renderOrder-e.renderOrder:n.material.id!==e.material.id?n.material.id-e.material.id:n.materialVariant!==e.materialVariant?n.materialVariant-e.materialVariant:n.z!==e.z?n.z-e.z:n.id-e.id}function vm(n,e){return n.groupOrder!==e.groupOrder?n.groupOrder-e.groupOrder:n.renderOrder!==e.renderOrder?n.renderOrder-e.renderOrder:n.z!==e.z?e.z-n.z:n.id-e.id}function Am(){let n=[],e=0,t=[],i=[],s=[];function r(){e=0,t.length=0,i.length=0,s.length=0}function o(h){let f=0;return h.isInstancedMesh&&(f+=2),h.isSkinnedMesh&&(f+=1),f}function a(h,f,g,y,m,p){let E=n[e];return E===void 0?(E={id:h.id,object:h,geometry:f,material:g,materialVariant:o(h),groupOrder:y,renderOrder:h.renderOrder,z:m,group:p},n[e]=E):(E.id=h.id,E.object=h,E.geometry=f,E.material=g,E.materialVariant=o(h),E.groupOrder=y,E.renderOrder=h.renderOrder,E.z=m,E.group=p),e++,E}function l(h,f,g,y,m,p,E){E.reversedDepth===!0&&(m=-m);let w=a(h,f,g,y,m,p);g.transmission>0?i.push(w):g.transparent===!0?s.push(w):t.push(w)}function c(h,f,g,y,m,p){let E=a(h,f,g,y,m,p);g.transmission>0?i.unshift(E):g.transparent===!0?s.unshift(E):t.unshift(E)}function u(h,f){t.length>1&&t.sort(h||DA),i.length>1&&i.sort(f||vm),s.length>1&&s.sort(f||vm)}function d(){for(let h=e,f=n.length;h<f;h++){let g=n[h];if(g.id===null)break;g.id=null,g.object=null,g.geometry=null,g.material=null,g.group=null}}return{opaque:t,transmissive:i,transparent:s,init:r,push:l,unshift:c,finish:d,sort:u}}function PA(){let n=new WeakMap;function e(i,s){let r=n.get(i),o;return r===void 0?(o=new Am,n.set(i,[o])):s>=r.length?(o=new Am,r.push(o)):o=r[s],o}function t(){n=new WeakMap}return{get:e,dispose:t}}function GA(){let n={};return{get:function(e){if(n[e.id]!==void 0)return n[e.id];let t;switch(e.type){case"SunLight":case"DirectionalLight":t={direction:new L,color:new He};break;case"SpotLight":t={position:new L,direction:new L,color:new He,distance:0,coneCos:0,penumbraCos:0,decay:0};break;case"PointLight":t={position:new L,color:new He,distance:0,decay:0};break;case"HemisphereLight":t={direction:new L,skyColor:new He,groundColor:new He};break;case"RectAreaLight":t={color:new He,position:new L,halfWidth:new L,halfHeight:new L};break}return n[e.id]=t,t}}}function VA(){let n={};return{get:function(e){if(n[e.id]!==void 0)return n[e.id];let t;switch(e.type){case"SunLight":case"DirectionalLight":t={shadowIntensity:1,shadowBias:0,shadowNormalBias:0,shadowRadius:1,shadowMapSize:new ze};break;case"SpotLight":t={shadowIntensity:1,shadowBias:0,shadowNormalBias:0,shadowRadius:1,shadowMapSize:new ze};break;case"PointLight":t={shadowIntensity:1,shadowBias:0,shadowNormalBias:0,shadowRadius:1,shadowMapSize:new ze,shadowCameraNear:1,shadowCameraFar:1e3};break}return n[e.id]=t,t}}}var OA=0;function kA(n,e){return(e.castShadow?2:0)-(n.castShadow?2:0)+(e.map?1:0)-(n.map?1:0)}function zA(n){let e=new GA,t=VA(),i={version:0,hash:{sunLength:-1,directionalLength:-1,pointLength:-1,spotLength:-1,rectAreaLength:-1,hemiLength:-1,numSunShadows:-1,numDirectionalShadows:-1,numPointShadows:-1,numSpotShadows:-1,numSpotMaps:-1,numLightProbes:-1},ambient:[0,0,0],probe:[],sun:[],sunShadow:[],sunShadowMap:[],sunShadowMatrix:[],sunShadowCascade:[],directional:[],directionalShadow:[],directionalShadowMap:[],directionalShadowMatrix:[],spot:[],spotLightMap:[],spotShadow:[],spotShadowMap:[],spotLightMatrix:[],rectArea:[],rectAreaLTC1:null,rectAreaLTC2:null,point:[],pointShadow:[],pointShadowMap:[],pointShadowMatrix:[],hemi:[],numSpotLightShadowsWithMaps:0,numLightProbes:0};for(let c=0;c<9;c++)i.probe.push(new L);let s=new L,r=new ct,o=new ct;function a(c){let u=0,d=0,h=0;for(let k=0;k<9;k++)i.probe[k].set(0,0,0);let f=0,g=0,y=0,m=0,p=0,E=0,w=0,S=0,v=0,A=0,R=0,b=0,M=0,B=0;c.sort(kA);for(let k=0,H=c.length;k<H;k++){let F=c[k],O=F.color,q=F.intensity,X=F.distance,re=null;if(F.shadow&&F.shadow.map&&(F.shadow.map.texture.format===Qn?re=F.shadow.map.texture:re=F.shadow.map.depthTexture||F.shadow.map.texture),F.isAmbientLight)u+=O.r*q,d+=O.g*q,h+=O.b*q;else if(F.isLightProbe){for(let K=0;K<9;K++)i.probe[K].addScaledVector(F.sh.coefficients[K],q);B++}else if(F.isSunLight){let K=e.get(F);if(K.color.copy(F.color).multiplyScalar(F.intensity),F.castShadow){let ne=F.shadow,se=t.get(F);se.shadowIntensity=ne.intensity,se.shadowBias=ne.bias,se.shadowNormalBias=ne.normalBias,se.shadowRadius=ne.radius,se.shadowMapSize.copy(ne.mapSize).multiply(ne.getFrameExtents()),i.sunShadow[g]=se,i.sunShadowMap[g]=re;let Ue=ne.getViewportCount();for(let Fe=0;Fe<Ue;Fe++)i.sunShadowMatrix[y+Fe]=ne.getMatrix(Fe),i.sunShadowCascade[y+Fe]=ne._cascadeData[Fe];y+=Ue,g++}i.sun[f]=K,f++}else if(F.isDirectionalLight){let K=e.get(F);if(K.color.copy(F.color).multiplyScalar(F.intensity),F.castShadow){let ne=F.shadow,se=t.get(F);se.shadowIntensity=ne.intensity,se.shadowBias=ne.bias,se.shadowNormalBias=ne.normalBias,se.shadowRadius=ne.radius,se.shadowMapSize=ne.mapSize,i.directionalShadow[m]=se,i.directionalShadowMap[m]=re,i.directionalShadowMatrix[m]=F.shadow.matrix,v++}i.directional[m]=K,m++}else if(F.isSpotLight){let K=e.get(F);K.position.setFromMatrixPosition(F.matrixWorld),K.color.copy(O).multiplyScalar(q),K.distance=X,K.coneCos=Math.cos(F.angle),K.penumbraCos=Math.cos(F.angle*(1-F.penumbra)),K.decay=F.decay,i.spot[E]=K;let ne=F.shadow;if(F.map&&(i.spotLightMap[b]=F.map,b++,ne.updateMatrices(F),F.castShadow&&M++),i.spotLightMatrix[E]=ne.matrix,F.castShadow){let se=t.get(F);se.shadowIntensity=ne.intensity,se.shadowBias=ne.bias,se.shadowNormalBias=ne.normalBias,se.shadowRadius=ne.radius,se.shadowMapSize=ne.mapSize,i.spotShadow[E]=se,i.spotShadowMap[E]=re,R++}E++}else if(F.isRectAreaLight){let K=e.get(F);K.color.copy(O).multiplyScalar(q),K.halfWidth.set(F.width*.5,0,0),K.halfHeight.set(0,F.height*.5,0),i.rectArea[w]=K,w++}else if(F.isPointLight){let K=e.get(F);if(K.color.copy(F.color).multiplyScalar(F.intensity),K.distance=F.distance,K.decay=F.decay,F.castShadow){let ne=F.shadow,se=t.get(F);se.shadowIntensity=ne.intensity,se.shadowBias=ne.bias,se.shadowNormalBias=ne.normalBias,se.shadowRadius=ne.radius,se.shadowMapSize=ne.mapSize,se.shadowCameraNear=ne.camera.near,se.shadowCameraFar=ne.camera.far,i.pointShadow[p]=se,i.pointShadowMap[p]=re,i.pointShadowMatrix[p]=F.shadow.matrix,A++}i.point[p]=K,p++}else if(F.isHemisphereLight){let K=e.get(F);K.skyColor.copy(F.color).multiplyScalar(q),K.groundColor.copy(F.groundColor).multiplyScalar(q),i.hemi[S]=K,S++}}w>0&&(n.has("OES_texture_float_linear")===!0?(i.rectAreaLTC1=me.LTC_FLOAT_1,i.rectAreaLTC2=me.LTC_FLOAT_2):(i.rectAreaLTC1=me.LTC_HALF_1,i.rectAreaLTC2=me.LTC_HALF_2)),i.ambient[0]=u,i.ambient[1]=d,i.ambient[2]=h;let D=i.hash;(D.sunLength!==f||D.directionalLength!==m||D.pointLength!==p||D.spotLength!==E||D.rectAreaLength!==w||D.hemiLength!==S||D.numSunShadows!==g||D.numDirectionalShadows!==v||D.numPointShadows!==A||D.numSpotShadows!==R||D.numSpotMaps!==b||D.numLightProbes!==B)&&(i.sun.length=f,i.directional.length=m,i.spot.length=E,i.rectArea.length=w,i.point.length=p,i.hemi.length=S,i.sunShadow.length=g,i.sunShadowMap.length=g,i.sunShadowMatrix.length=y,i.sunShadowCascade.length=y,i.directionalShadow.length=v,i.directionalShadowMap.length=v,i.directionalShadowMatrix.length=v,i.pointShadow.length=A,i.pointShadowMap.length=A,i.pointShadowMatrix.length=A,i.spotShadow.length=R,i.spotShadowMap.length=R,i.spotLightMatrix.length=R+b-M,i.spotLightMap.length=b,i.numSpotLightShadowsWithMaps=M,i.numLightProbes=B,D.sunLength=f,D.directionalLength=m,D.pointLength=p,D.spotLength=E,D.rectAreaLength=w,D.hemiLength=S,D.numSunShadows=g,D.numDirectionalShadows=v,D.numPointShadows=A,D.numSpotShadows=R,D.numSpotMaps=b,D.numLightProbes=B,i.version=OA++)}function l(c,u){let d=0,h=0,f=0,g=0,y=0,m=0,p=u.matrixWorldInverse;for(let E=0,w=c.length;E<w;E++){let S=c[E];if(S.isSunLight){let v=i.sun[d];v.direction.setFromMatrixPosition(S.matrixWorld),v.direction.transformDirection(p),d++}else if(S.isDirectionalLight){let v=i.directional[h];v.direction.setFromMatrixPosition(S.matrixWorld),s.setFromMatrixPosition(S.target.matrixWorld),v.direction.sub(s),v.direction.transformDirection(p),h++}else if(S.isSpotLight){let v=i.spot[g];v.position.setFromMatrixPosition(S.matrixWorld),v.position.applyMatrix4(p),v.direction.setFromMatrixPosition(S.matrixWorld),s.setFromMatrixPosition(S.target.matrixWorld),v.direction.sub(s),v.direction.transformDirection(p),g++}else if(S.isRectAreaLight){let v=i.rectArea[y];v.position.setFromMatrixPosition(S.matrixWorld),v.position.applyMatrix4(p),o.identity(),r.copy(S.matrixWorld),r.premultiply(p),o.extractRotation(r),v.halfWidth.set(S.width*.5,0,0),v.halfHeight.set(0,S.height*.5,0),v.halfWidth.applyMatrix4(o),v.halfHeight.applyMatrix4(o),y++}else if(S.isPointLight){let v=i.point[f];v.position.setFromMatrixPosition(S.matrixWorld),v.position.applyMatrix4(p),f++}else if(S.isHemisphereLight){let v=i.hemi[m];v.direction.setFromMatrixPosition(S.matrixWorld),v.direction.transformDirection(p),m++}}}return{setup:a,setupView:l,state:i}}function Tm(n){let e=new zA(n),t=[],i=[],s=[];function r(h){d.camera=h,t.length=0,i.length=0,s.length=0}function o(h){t.push(h)}function a(h){i.push(h)}function l(h){s.push(h)}function c(){e.setup(t)}function u(h){e.setupView(t,h)}let d={lightsArray:t,shadowsArray:i,lightProbeGridArray:s,camera:null,lights:e,transmissionRenderTarget:{},textureUnits:0};return{init:r,state:d,setupLights:c,setupLightsView:u,pushLight:o,pushShadow:a,pushLightProbeGrid:l}}function WA(n){let e=new WeakMap;function t(s,r=0){let o=e.get(s),a;return o===void 0?(a=new Tm(n),e.set(s,[a])):r>=o.length?(a=new Tm(n),o.push(a)):a=o[r],a}function i(){e=new WeakMap}return{get:t,dispose:i}}var HA=`void main() {
	gl_Position = vec4( position, 1.0 );
}`,YA=`uniform sampler2D shadow_pass;
uniform vec2 resolution;
uniform float radius;
void main() {
	const float samples = float( VSM_SAMPLES );
	float mean = 0.0;
	float squared_mean = 0.0;
	float uvStride = samples <= 1.0 ? 0.0 : 2.0 / ( samples - 1.0 );
	float uvStart = samples <= 1.0 ? 0.0 : - 1.0;
	for ( float i = 0.0; i < samples; i ++ ) {
		float uvOffset = uvStart + i * uvStride;
		#ifdef HORIZONTAL_PASS
			vec2 distribution = texture2D( shadow_pass, ( gl_FragCoord.xy + vec2( uvOffset, 0.0 ) * radius ) / resolution ).rg;
			mean += distribution.x;
			squared_mean += distribution.y * distribution.y + distribution.x * distribution.x;
		#else
			float depth = texture2D( shadow_pass, ( gl_FragCoord.xy + vec2( 0.0, uvOffset ) * radius ) / resolution ).r;
			mean += depth;
			squared_mean += depth * depth;
		#endif
	}
	mean = mean / samples;
	squared_mean = squared_mean / samples;
	float std_dev = sqrt( max( 0.0, squared_mean - mean * mean ) );
	gl_FragColor = vec4( mean, std_dev, 0.0, 1.0 );
}`,ZA=[new L(1,0,0),new L(-1,0,0),new L(0,1,0),new L(0,-1,0),new L(0,0,1),new L(0,0,-1)],XA=[new L(0,-1,0),new L(0,-1,0),new L(0,0,1),new L(0,0,-1),new L(0,-1,0),new L(0,-1,0)],Em=new ct,xa=new L,Wh=new L;function KA(n,e,t){let i=new ms,s=new ze,r=new ze,o=new dt,a=new Nl,l=new Il,c={},u=t.maxTextureSize,d={[Jn]:Dt,[Dt]:Jn,[sn]:sn},h=new Cn({defines:{VSM_SAMPLES:8},uniforms:{shadow_pass:{value:null},resolution:{value:new ze},radius:{value:4}},vertexShader:HA,fragmentShader:YA}),f=h.clone();f.defines.HORIZONTAL_PASS=1;let g=new dn;g.setAttribute("position",new Xt(new Float32Array([-1,-1,.5,3,-1,.5,-1,3,.5]),3));let y=new fn(g,h),m=this;this.enabled=!1,this.autoUpdate=!0,this.needsUpdate=!1,this.type=Ar;let p=this.type;this.render=function(A,R,b){if(m.enabled===!1||m.autoUpdate===!1&&m.needsUpdate===!1||A.length===0)return;this.type===ih&&(pe("WebGLShadowMap: PCFSoftShadowMap has been removed. Using PCFShadowMap instead."),this.type=Ar);let M=n.getRenderTarget(),B=n.getActiveCubeFace(),D=n.getActiveMipmapLevel(),k=n.state;k.setBlending(qn),k.buffers.depth.getReversed()===!0?k.buffers.color.setClear(0,0,0,0):k.buffers.color.setClear(1,1,1,1),k.buffers.depth.setTest(!0),k.setScissorTest(!1);let H=p!==this.type;H&&R.traverse(function(F){F.material&&(Array.isArray(F.material)?F.material.forEach(O=>O.needsUpdate=!0):F.material.needsUpdate=!0)});for(let F=0,O=A.length;F<O;F++){let q=A[F],X=q.shadow;if(X===void 0){pe("WebGLShadowMap:",q,"has no shadow.");continue}if(X.autoUpdate===!1&&X.needsUpdate===!1)continue;s.copy(X.mapSize);let re=X.getFrameExtents();s.multiply(re),r.copy(X.mapSize),(s.x>u||s.y>u)&&(s.x>u&&(r.x=Math.floor(u/re.x),s.x=r.x*re.x,X.mapSize.x=r.x),s.y>u&&(r.y=Math.floor(u/re.y),s.y=r.y*re.y,X.mapSize.y=r.y));let K=n.state.buffers.depth.getReversed();if(X.camera._reversedDepth=K,X.map===null||H===!0){if(X.map!==null&&(X.map.depthTexture!==null&&(X.map.depthTexture.dispose(),X.map.depthTexture=null),X.map.dispose()),this.type===bs){if(q.isPointLight){pe("WebGLShadowMap: VSM shadow maps are not supported for PointLights. Use PCF or BasicShadowMap instead.");continue}X.map=new _n(s.x,s.y,{format:Qn,type:Sn,minFilter:Bt,magFilter:Bt,generateMipmaps:!1}),X.map.texture.name=q.name+".shadowMap",X.map.depthTexture=new $n(s.x,s.y,Dn),X.map.depthTexture.name=q.name+".shadowMapDepth",X.map.depthTexture.format=Un,X.map.depthTexture.compareFunction=null,X.map.depthTexture.minFilter=Rt,X.map.depthTexture.magFilter=Rt}else q.isPointLight?(X.map=new gc(s.x),X.map.depthTexture=new bo(s.x,bn)):(X.map=new _n(s.x,s.y),X.map.depthTexture=new $n(s.x,s.y,bn)),X.map.depthTexture.name=q.name+".shadowMap",X.map.depthTexture.format=Un,this.type===Ar?(X.map.depthTexture.compareFunction=K?$i:ws,X.map.depthTexture.minFilter=Bt,X.map.depthTexture.magFilter=Bt):(X.map.depthTexture.compareFunction=null,X.map.depthTexture.minFilter=Rt,X.map.depthTexture.magFilter=Rt);X.camera.updateProjectionMatrix()}X.map.isWebGLCubeRenderTarget!==!0&&(X.map.width!==s.x||X.map.height!==s.y)&&X.map.setSize(s.x,s.y);let ne=X.map.isWebGLCubeRenderTarget?6:X.getViewportCount();q.isPointLight!==!0&&X.updateMatrices(q,b);for(let se=0;se<ne;se++){let Ue=X.getCamera(se);if(q.isPointLight){let Fe=X.camera,yt=X.matrix,je=q.distance||Fe.far;je!==Fe.far&&(Fe.far=je,Fe.updateProjectionMatrix()),xa.setFromMatrixPosition(q.matrixWorld),Fe.position.copy(xa),Wh.copy(Fe.position),Wh.add(ZA[se]),Fe.up.copy(XA[se]),Fe.lookAt(Wh),Fe.updateMatrixWorld(),yt.makeTranslation(-xa.x,-xa.y,-xa.z),Em.multiplyMatrices(Fe.projectionMatrix,Fe.matrixWorldInverse),X._frustum.setFromProjectionMatrix(Em,Fe.coordinateSystem,Fe.reversedDepth)}if(X.map.isWebGLCubeRenderTarget)n.setRenderTarget(X.map,se),n.clear();else{se===0&&(n.setRenderTarget(X.map),n.clear());let Fe=X.getViewport(se);o.set(r.x*Fe.x,r.y*Fe.y,r.x*Fe.z,r.y*Fe.w),k.viewport(o)}i=X.getFrustum(se),S(R,b,Ue,q,this.type)}X.isPointLightShadow!==!0&&this.type===bs&&E(X,b),X.needsUpdate=!1}p=this.type,m.needsUpdate=!1,n.setRenderTarget(M,B,D)};function E(A,R){let b=e.update(y);h.defines.VSM_SAMPLES!==A.blurSamples&&(h.defines.VSM_SAMPLES=A.blurSamples,f.defines.VSM_SAMPLES=A.blurSamples,h.needsUpdate=!0,f.needsUpdate=!0),A.mapPass===null?A.mapPass=new _n(s.x,s.y,{format:Qn,type:Sn}):(A.mapPass.width!==A.map.width||A.mapPass.height!==A.map.height)&&A.mapPass.setSize(A.map.width,A.map.height),h.uniforms.shadow_pass.value=A.map.depthTexture,h.uniforms.resolution.value.set(A.map.width,A.map.height),h.uniforms.radius.value=A.radius,n.setRenderTarget(A.mapPass),n.clear(),n.renderBufferDirect(R,null,b,h,y,null),f.uniforms.shadow_pass.value=A.mapPass.texture,f.uniforms.resolution.value.set(A.map.width,A.map.height),f.uniforms.radius.value=A.radius,n.setRenderTarget(A.map),n.clear(),n.renderBufferDirect(R,null,b,f,y,null)}function w(A,R,b,M){let B=null,D=b.isPointLight===!0?A.customDistanceMaterial:A.customDepthMaterial;if(D!==void 0)B=D;else if(B=b.isPointLight===!0?l:a,n.localClippingEnabled&&R.clipShadows===!0&&Array.isArray(R.clippingPlanes)&&R.clippingPlanes.length!==0||R.displacementMap&&R.displacementScale!==0||R.alphaMap&&R.alphaTest>0||R.map&&R.alphaTest>0||R.alphaToCoverage===!0){let k=B.uuid,H=R.uuid,F=c[k];F===void 0&&(F={},c[k]=F);let O=F[H];O===void 0&&(O=B.clone(),F[H]=O,R.addEventListener("dispose",v)),B=O}if(B.visible=R.visible,B.wireframe=R.wireframe,M===bs?B.side=R.shadowSide!==null?R.shadowSide:R.side:B.side=R.shadowSide!==null?R.shadowSide:d[R.side],B.alphaMap=R.alphaMap,B.alphaTest=R.alphaToCoverage===!0?.5:R.alphaTest,B.map=R.map,B.clipShadows=R.clipShadows,B.clippingPlanes=R.clippingPlanes,B.clipIntersection=R.clipIntersection,B.displacementMap=R.displacementMap,B.displacementScale=R.displacementScale,B.displacementBias=R.displacementBias,B.wireframeLinewidth=R.wireframeLinewidth,B.linewidth=R.linewidth,b.isPointLight===!0&&B.isMeshDistanceMaterial===!0){let k=n.properties.get(B);k.light=b}return B}function S(A,R,b,M,B){if(A.visible===!1)return;if(A.layers.test(R.layers)&&(A.isMesh||A.isLine||A.isPoints)&&(A.castShadow||A.receiveShadow&&B===bs)&&(!A.frustumCulled||A.intersectsFrustum(i))){A.modelViewMatrix.multiplyMatrices(b.matrixWorldInverse,A.matrixWorld);let H=e.update(A),F=A.material;if(Array.isArray(F)){let O=H.groups;for(let q=0,X=O.length;q<X;q++){let re=O[q],K=F[re.materialIndex];if(K&&K.visible){let ne=w(A,K,M,B);A.onBeforeShadow(n,A,R,b,H,ne,re),n.renderBufferDirect(b,null,H,ne,A,re),A.onAfterShadow(n,A,R,b,H,ne,re)}}}else if(F.visible){let O=w(A,F,M,B);A.onBeforeShadow(n,A,R,b,H,O,null),n.renderBufferDirect(b,null,H,O,A,null),A.onAfterShadow(n,A,R,b,H,O,null)}}let k=A.children;for(let H=0,F=k.length;H<F;H++)S(k[H],R,b,M,B)}function v(A){A.target.removeEventListener("dispose",v);for(let b in c){let M=c[b],B=A.target.uuid;B in M&&(M[B].dispose(),delete M[B])}}}function $A(n,e){function t(){let N=!1,he=new dt,Q=null,de=new dt(0,0,0,0);return{setMask:function(be){Q!==be&&!N&&(n.colorMask(be,be,be,be),Q=be)},setLocked:function(be){N=be},setClear:function(be,ie,Le,Re,bt){bt===!0&&(be*=Re,ie*=Re,Le*=Re),he.set(be,ie,Le,Re),de.equals(he)===!1&&(n.clearColor(be,ie,Le,Re),de.copy(he))},reset:function(){N=!1,Q=null,de.set(-1,0,0,0)}}}function i(){let N=!1,he=!1,Q=null,de=null,be=null;return{setReversed:function(ie){if(he!==ie){let Le=e.get("EXT_clip_control");ie?Le.clipControlEXT(Le.LOWER_LEFT_EXT,Le.ZERO_TO_ONE_EXT):Le.clipControlEXT(Le.LOWER_LEFT_EXT,Le.NEGATIVE_ONE_TO_ONE_EXT),he=ie;let Re=be;be=null,this.setClear(Re)}},getReversed:function(){return he},setTest:function(ie){ie?te(n.DEPTH_TEST):Te(n.DEPTH_TEST)},setMask:function(ie){Q!==ie&&!N&&(n.depthMask(ie),Q=ie)},setFunc:function(ie){if(he&&(ie=Bh[ie]),de!==ie){switch(ie){case ro:n.depthFunc(n.NEVER);break;case oo:n.depthFunc(n.ALWAYS);break;case ao:n.depthFunc(n.LESS);break;case us:n.depthFunc(n.LEQUAL);break;case lo:n.depthFunc(n.EQUAL);break;case co:n.depthFunc(n.GEQUAL);break;case uo:n.depthFunc(n.GREATER);break;case ho:n.depthFunc(n.NOTEQUAL);break;default:n.depthFunc(n.LEQUAL)}de=ie}},setLocked:function(ie){N=ie},setClear:function(ie){be!==ie&&(be=ie,he&&(ie=1-ie),n.clearDepth(ie))},reset:function(){N=!1,Q=null,de=null,be=null,he=!1}}}function s(){let N=!1,he=null,Q=null,de=null,be=null,ie=null,Le=null,Re=null,bt=null;return{setTest:function(at){N||(at?te(n.STENCIL_TEST):Te(n.STENCIL_TEST))},setMask:function(at){he!==at&&!N&&(n.stencilMask(at),he=at)},setFunc:function(at,On,ri){(Q!==at||de!==On||be!==ri)&&(n.stencilFunc(at,On,ri),Q=at,de=On,be=ri)},setOp:function(at,On,ri){(ie!==at||Le!==On||Re!==ri)&&(n.stencilOp(at,On,ri),ie=at,Le=On,Re=ri)},setLocked:function(at){N=at},setClear:function(at){bt!==at&&(n.clearStencil(at),bt=at)},reset:function(){N=!1,he=null,Q=null,de=null,be=null,ie=null,Le=null,Re=null,bt=null}}}let r=new t,o=new i,a=new s,l=new WeakMap,c=new WeakMap,u={},d={},h={},f=new WeakMap,g=[],y=null,m=!1,p=null,E=null,w=null,S=null,v=null,A=null,R=null,b=new He(0,0,0),M=0,B=!1,D=null,k=null,H=null,F=null,O=null,q=n.getParameter(n.MAX_COMBINED_TEXTURE_IMAGE_UNITS),X=!1,re=0,K=n.getParameter(n.VERSION);K.indexOf("WebGL")!==-1?(re=parseFloat(/^WebGL (\d)/.exec(K)[1]),X=re>=1):K.indexOf("OpenGL ES")!==-1&&(re=parseFloat(/^OpenGL ES (\d)/.exec(K)[1]),X=re>=2);let ne=null,se={},Ue=n.getParameter(n.SCISSOR_BOX),Fe=n.getParameter(n.VIEWPORT),yt=new dt().fromArray(Ue),je=new dt().fromArray(Fe);function rt(N,he,Q,de){let be=new Uint8Array(4),ie=n.createTexture();n.bindTexture(N,ie),n.texParameteri(N,n.TEXTURE_MIN_FILTER,n.NEAREST),n.texParameteri(N,n.TEXTURE_MAG_FILTER,n.NEAREST);for(let Le=0;Le<Q;Le++)N===n.TEXTURE_3D||N===n.TEXTURE_2D_ARRAY?n.texImage3D(he,0,n.RGBA,1,1,de,0,n.RGBA,n.UNSIGNED_BYTE,be):n.texImage2D(he+Le,0,n.RGBA,1,1,0,n.RGBA,n.UNSIGNED_BYTE,be);return ie}let $={};$[n.TEXTURE_2D]=rt(n.TEXTURE_2D,n.TEXTURE_2D,1),$[n.TEXTURE_CUBE_MAP]=rt(n.TEXTURE_CUBE_MAP,n.TEXTURE_CUBE_MAP_POSITIVE_X,6),$[n.TEXTURE_2D_ARRAY]=rt(n.TEXTURE_2D_ARRAY,n.TEXTURE_2D_ARRAY,1,1),$[n.TEXTURE_3D]=rt(n.TEXTURE_3D,n.TEXTURE_3D,1,1),r.setClear(0,0,0,1),o.setClear(1),a.setClear(0),te(n.DEPTH_TEST),o.setFunc(us),Qe(!1),Tt(Wl),te(n.CULL_FACE),st(qn);function te(N){u[N]!==!0&&(n.enable(N),u[N]=!0)}function Te(N){u[N]!==!1&&(n.disable(N),u[N]=!1)}function Oe(N,he){return h[N]!==he?(n.bindFramebuffer(N,he),h[N]=he,N===n.DRAW_FRAMEBUFFER&&(h[n.FRAMEBUFFER]=he),N===n.FRAMEBUFFER&&(h[n.DRAW_FRAMEBUFFER]=he),!0):!1}function ve(N,he){let Q=g,de=!1;if(N){Q=f.get(he),Q===void 0&&(Q=[],f.set(he,Q));let be=N.textures;if(Q.length!==be.length||Q[0]!==n.COLOR_ATTACHMENT0){for(let ie=0,Le=be.length;ie<Le;ie++)Q[ie]=n.COLOR_ATTACHMENT0+ie;Q.length=be.length,de=!0}}else Q[0]!==n.BACK&&(Q[0]=n.BACK,de=!0);de&&n.drawBuffers(Q)}function Ke(N){return y!==N?(n.useProgram(N),y=N,!0):!1}let Ot={[Xi]:n.FUNC_ADD,[rh]:n.FUNC_SUBTRACT,[oh]:n.FUNC_REVERSE_SUBTRACT};Ot[ah]=n.MIN,Ot[lh]=n.MAX;let $e={[ch]:n.ZERO,[uh]:n.ONE,[hh]:n.SRC_COLOR,[Xl]:n.SRC_ALPHA,[xh]:n.SRC_ALPHA_SATURATE,[mh]:n.DST_COLOR,[fh]:n.DST_ALPHA,[dh]:n.ONE_MINUS_SRC_COLOR,[Kl]:n.ONE_MINUS_SRC_ALPHA,[gh]:n.ONE_MINUS_DST_COLOR,[ph]:n.ONE_MINUS_DST_ALPHA,[yh]:n.CONSTANT_COLOR,[_h]:n.ONE_MINUS_CONSTANT_COLOR,[bh]:n.CONSTANT_ALPHA,[Sh]:n.ONE_MINUS_CONSTANT_ALPHA};function st(N,he,Q,de,be,ie,Le,Re,bt,at){if(N===qn){m===!0&&(Te(n.BLEND),m=!1);return}if(m===!1&&(te(n.BLEND),m=!0),N!==sh){if(N!==p||at!==B){if((E!==Xi||v!==Xi)&&(n.blendEquation(n.FUNC_ADD),E=Xi,v=Xi),at)switch(N){case Ss:n.blendFuncSeparate(n.ONE,n.ONE_MINUS_SRC_ALPHA,n.ONE,n.ONE_MINUS_SRC_ALPHA);break;case Hl:n.blendFunc(n.ONE,n.ONE);break;case Yl:n.blendFuncSeparate(n.ZERO,n.ONE_MINUS_SRC_COLOR,n.ZERO,n.ONE);break;case Zl:n.blendFuncSeparate(n.DST_COLOR,n.ONE_MINUS_SRC_ALPHA,n.ZERO,n.ONE);break;default:_e("WebGLState: Invalid blending: ",N);break}else switch(N){case Ss:n.blendFuncSeparate(n.SRC_ALPHA,n.ONE_MINUS_SRC_ALPHA,n.ONE,n.ONE_MINUS_SRC_ALPHA);break;case Hl:n.blendFuncSeparate(n.SRC_ALPHA,n.ONE,n.ONE,n.ONE);break;case Yl:_e("WebGLState: SubtractiveBlending requires material.premultipliedAlpha = true");break;case Zl:_e("WebGLState: MultiplyBlending requires material.premultipliedAlpha = true");break;default:_e("WebGLState: Invalid blending: ",N);break}w=null,S=null,A=null,R=null,b.set(0,0,0),M=0,p=N,B=at}return}be=be||he,ie=ie||Q,Le=Le||de,(he!==E||be!==v)&&(n.blendEquationSeparate(Ot[he],Ot[be]),E=he,v=be),(Q!==w||de!==S||ie!==A||Le!==R)&&(n.blendFuncSeparate($e[Q],$e[de],$e[ie],$e[Le]),w=Q,S=de,A=ie,R=Le),(Re.equals(b)===!1||bt!==M)&&(n.blendColor(Re.r,Re.g,Re.b,bt),b.copy(Re),M=bt),p=N,B=!1}function _t(N,he){N.side===sn?Te(n.CULL_FACE):te(n.CULL_FACE);let Q=N.side===Dt;he&&(Q=!Q),Qe(Q),N.blending===Ss&&N.transparent===!1?st(qn):st(N.blending,N.blendEquation,N.blendSrc,N.blendDst,N.blendEquationAlpha,N.blendSrcAlpha,N.blendDstAlpha,N.blendColor,N.blendAlpha,N.premultipliedAlpha),o.setFunc(N.depthFunc),o.setTest(N.depthTest),o.setMask(N.depthWrite),r.setMask(N.colorWrite);let de=N.stencilWrite;a.setTest(de),de&&(a.setMask(N.stencilWriteMask),a.setFunc(N.stencilFunc,N.stencilRef,N.stencilFuncMask),a.setOp(N.stencilFail,N.stencilZFail,N.stencilZPass)),xn(N.polygonOffset,N.polygonOffsetFactor,N.polygonOffsetUnits),N.alphaToCoverage===!0?te(n.SAMPLE_ALPHA_TO_COVERAGE):Te(n.SAMPLE_ALPHA_TO_COVERAGE)}function Qe(N){D!==N&&(N?n.frontFace(n.CW):n.frontFace(n.CCW),D=N)}function Tt(N){N!==th?(te(n.CULL_FACE),N!==k&&(N===Wl?n.cullFace(n.BACK):N===nh?n.cullFace(n.FRONT):n.cullFace(n.FRONT_AND_BACK))):Te(n.CULL_FACE),k=N}function Yt(N){N!==H&&(X&&n.lineWidth(N),H=N)}function xn(N,he,Q){N?(te(n.POLYGON_OFFSET_FILL),(F!==he||O!==Q)&&(F=he,O=Q,o.getReversed()&&(he=-he),n.polygonOffset(he,Q))):Te(n.POLYGON_OFFSET_FILL)}function Mt(N){N?te(n.SCISSOR_TEST):Te(n.SCISSOR_TEST)}function Ft(N){N===void 0&&(N=n.TEXTURE0+q-1),ne!==N&&(n.activeTexture(N),ne=N)}function I(N,he,Q){Q===void 0&&(ne===null?Q=n.TEXTURE0+q-1:Q=ne);let de=se[Q];de===void 0&&(de={type:void 0,texture:void 0},se[Q]=de),(de.type!==N||de.texture!==he)&&(ne!==Q&&(n.activeTexture(Q),ne=Q),n.bindTexture(N,he||$[N]),de.type=N,de.texture=he)}function Qt(){let N=se[ne];N!==void 0&&N.type!==void 0&&(n.bindTexture(N.type,null),N.type=void 0,N.texture=void 0)}function ut(){try{n.compressedTexImage2D(...arguments)}catch(N){_e("WebGLState:",N)}}function T(){try{n.compressedTexImage3D(...arguments)}catch(N){_e("WebGLState:",N)}}function x(){try{n.texSubImage2D(...arguments)}catch(N){_e("WebGLState:",N)}}function U(){try{n.texSubImage3D(...arguments)}catch(N){_e("WebGLState:",N)}}function z(){try{n.compressedTexSubImage2D(...arguments)}catch(N){_e("WebGLState:",N)}}function Z(){try{n.compressedTexSubImage3D(...arguments)}catch(N){_e("WebGLState:",N)}}function ae(){try{n.texStorage2D(...arguments)}catch(N){_e("WebGLState:",N)}}function le(){try{n.texStorage3D(...arguments)}catch(N){_e("WebGLState:",N)}}function J(){try{n.texImage2D(...arguments)}catch(N){_e("WebGLState:",N)}}function j(){try{n.texImage3D(...arguments)}catch(N){_e("WebGLState:",N)}}function ce(N){return d[N]!==void 0?d[N]:n.getParameter(N)}function Ie(N,he){d[N]!==he&&(n.pixelStorei(N,he),d[N]=he)}function fe(N){yt.equals(N)===!1&&(n.scissor(N.x,N.y,N.z,N.w),yt.copy(N))}function ue(N){je.equals(N)===!1&&(n.viewport(N.x,N.y,N.z,N.w),je.copy(N))}function Be(N,he){let Q=c.get(he);Q===void 0&&(Q=new WeakMap,c.set(he,Q));let de=Q.get(N);de===void 0&&(de=n.getUniformBlockIndex(he,N.name),Q.set(N,de))}function Pe(N,he){let de=c.get(he).get(N);l.get(he)!==de&&(n.uniformBlockBinding(he,de,N.__bindingPointIndex),l.set(he,de))}function We(){n.disable(n.BLEND),n.disable(n.CULL_FACE),n.disable(n.DEPTH_TEST),n.disable(n.POLYGON_OFFSET_FILL),n.disable(n.SCISSOR_TEST),n.disable(n.STENCIL_TEST),n.disable(n.SAMPLE_ALPHA_TO_COVERAGE),n.blendEquation(n.FUNC_ADD),n.blendFunc(n.ONE,n.ZERO),n.blendFuncSeparate(n.ONE,n.ZERO,n.ONE,n.ZERO),n.blendColor(0,0,0,0),n.colorMask(!0,!0,!0,!0),n.clearColor(0,0,0,0),n.depthMask(!0),n.depthFunc(n.LESS),o.setReversed(!1),n.clearDepth(1),n.stencilMask(4294967295),n.stencilFunc(n.ALWAYS,0,4294967295),n.stencilOp(n.KEEP,n.KEEP,n.KEEP),n.clearStencil(0),n.cullFace(n.BACK),n.frontFace(n.CCW),n.polygonOffset(0,0),n.activeTexture(n.TEXTURE0),n.bindFramebuffer(n.FRAMEBUFFER,null),n.bindFramebuffer(n.DRAW_FRAMEBUFFER,null),n.bindFramebuffer(n.READ_FRAMEBUFFER,null),n.useProgram(null),n.lineWidth(1),n.scissor(0,0,n.canvas.width,n.canvas.height),n.viewport(0,0,n.canvas.width,n.canvas.height),n.pixelStorei(n.PACK_ALIGNMENT,4),n.pixelStorei(n.UNPACK_ALIGNMENT,4),n.pixelStorei(n.UNPACK_FLIP_Y_WEBGL,!1),n.pixelStorei(n.UNPACK_PREMULTIPLY_ALPHA_WEBGL,!1),n.pixelStorei(n.UNPACK_COLORSPACE_CONVERSION_WEBGL,n.BROWSER_DEFAULT_WEBGL),n.pixelStorei(n.PACK_ROW_LENGTH,0),n.pixelStorei(n.PACK_SKIP_PIXELS,0),n.pixelStorei(n.PACK_SKIP_ROWS,0),n.pixelStorei(n.UNPACK_ROW_LENGTH,0),n.pixelStorei(n.UNPACK_IMAGE_HEIGHT,0),n.pixelStorei(n.UNPACK_SKIP_PIXELS,0),n.pixelStorei(n.UNPACK_SKIP_ROWS,0),n.pixelStorei(n.UNPACK_SKIP_IMAGES,0),u={},d={},ne=null,se={},h={},f=new WeakMap,g=[],y=null,m=!1,p=null,E=null,w=null,S=null,v=null,A=null,R=null,b=new He(0,0,0),M=0,B=!1,D=null,k=null,H=null,F=null,O=null,yt.set(0,0,n.canvas.width,n.canvas.height),je.set(0,0,n.canvas.width,n.canvas.height),r.reset(),o.reset(),a.reset()}return{buffers:{color:r,depth:o,stencil:a},enable:te,disable:Te,bindFramebuffer:Oe,drawBuffers:ve,useProgram:Ke,setBlending:st,setMaterial:_t,setFlipSided:Qe,setCullFace:Tt,setLineWidth:Yt,setPolygonOffset:xn,setScissorTest:Mt,activeTexture:Ft,bindTexture:I,unbindTexture:Qt,compressedTexImage2D:ut,compressedTexImage3D:T,texImage2D:J,texImage3D:j,pixelStorei:Ie,getParameter:ce,updateUBOMapping:Be,uniformBlockBinding:Pe,texStorage2D:ae,texStorage3D:le,texSubImage2D:x,texSubImage3D:U,compressedTexSubImage2D:z,compressedTexSubImage3D:Z,scissor:fe,viewport:ue,reset:We}}function JA(n,e,t,i,s,r,o){let a=e.has("WEBGL_multisampled_render_to_texture")?e.get("WEBGL_multisampled_render_to_texture"):null,l=typeof navigator>"u"?!1:/OculusBrowser/g.test(navigator.userAgent),c=new ze,u=new WeakMap,d=new Set,h,f=new WeakMap,g=!1;try{g=typeof OffscreenCanvas<"u"&&new OffscreenCanvas(1,1).getContext("2d")!==null}catch{}function y(T,x){return g?new OffscreenCanvas(T,x):po("canvas")}function m(T,x,U){let z=1,Z=ut(T);if((Z.width>U||Z.height>U)&&(z=U/Math.max(Z.width,Z.height)),z<1)if(typeof HTMLImageElement<"u"&&T instanceof HTMLImageElement||typeof HTMLCanvasElement<"u"&&T instanceof HTMLCanvasElement||typeof ImageBitmap<"u"&&T instanceof ImageBitmap||typeof VideoFrame<"u"&&T instanceof VideoFrame){let ae=Math.floor(z*Z.width),le=Math.floor(z*Z.height);h===void 0&&(h=y(ae,le));let J=x?y(ae,le):h;return J.width=ae,J.height=le,J.getContext("2d").drawImage(T,0,0,ae,le),pe("WebGLRenderer: Texture has been resized from ("+Z.width+"x"+Z.height+") to ("+ae+"x"+le+")."),J}else return"data"in T&&pe("WebGLRenderer: Image in DataTexture is too big ("+Z.width+"x"+Z.height+")."),T;return T}function p(T){return T.generateMipmaps}function E(T){n.generateMipmap(T)}function w(T){return T.isWebGLCubeRenderTarget?n.TEXTURE_CUBE_MAP:T.isWebGL3DRenderTarget?n.TEXTURE_3D:T.isWebGLArrayRenderTarget||T.isCompressedArrayTexture?n.TEXTURE_2D_ARRAY:n.TEXTURE_2D}function S(T,x,U,z,Z,ae=!1){if(T!==null){if(n[T]!==void 0)return n[T];pe("WebGLRenderer: Attempt to use non-existing WebGL internal format '"+T+"'")}let le;z&&(le=e.get("EXT_texture_norm16"),le||pe("WebGLRenderer: Unable to use normalized textures without EXT_texture_norm16 extension"));let J=x;if(x===n.RED&&(U===n.FLOAT&&(J=n.R32F),U===n.HALF_FLOAT&&(J=n.R16F),U===n.UNSIGNED_BYTE&&(J=n.R8),U===n.UNSIGNED_SHORT&&le&&(J=le.R16_EXT),U===n.SHORT&&le&&(J=le.R16_SNORM_EXT)),x===n.RED_INTEGER&&(U===n.UNSIGNED_BYTE&&(J=n.R8UI),U===n.UNSIGNED_SHORT&&(J=n.R16UI),U===n.UNSIGNED_INT&&(J=n.R32UI),U===n.BYTE&&(J=n.R8I),U===n.SHORT&&(J=n.R16I),U===n.INT&&(J=n.R32I)),x===n.RG&&(U===n.FLOAT&&(J=n.RG32F),U===n.HALF_FLOAT&&(J=n.RG16F),U===n.UNSIGNED_BYTE&&(J=n.RG8),U===n.UNSIGNED_SHORT&&le&&(J=le.RG16_EXT),U===n.SHORT&&le&&(J=le.RG16_SNORM_EXT)),x===n.RG_INTEGER&&(U===n.UNSIGNED_BYTE&&(J=n.RG8UI),U===n.UNSIGNED_SHORT&&(J=n.RG16UI),U===n.UNSIGNED_INT&&(J=n.RG32UI),U===n.BYTE&&(J=n.RG8I),U===n.SHORT&&(J=n.RG16I),U===n.INT&&(J=n.RG32I)),x===n.RGB_INTEGER&&(U===n.UNSIGNED_BYTE&&(J=n.RGB8UI),U===n.UNSIGNED_SHORT&&(J=n.RGB16UI),U===n.UNSIGNED_INT&&(J=n.RGB32UI),U===n.BYTE&&(J=n.RGB8I),U===n.SHORT&&(J=n.RGB16I),U===n.INT&&(J=n.RGB32I)),x===n.RGBA_INTEGER&&(U===n.UNSIGNED_BYTE&&(J=n.RGBA8UI),U===n.UNSIGNED_SHORT&&(J=n.RGBA16UI),U===n.UNSIGNED_INT&&(J=n.RGBA32UI),U===n.BYTE&&(J=n.RGBA8I),U===n.SHORT&&(J=n.RGBA16I),U===n.INT&&(J=n.RGBA32I)),x===n.RGB&&(U===n.UNSIGNED_SHORT&&le&&(J=le.RGB16_EXT),U===n.SHORT&&le&&(J=le.RGB16_SNORM_EXT),U===n.UNSIGNED_INT_5_9_9_9_REV&&(J=n.RGB9_E5),U===n.UNSIGNED_INT_10F_11F_11F_REV&&(J=n.R11F_G11F_B10F)),x===n.RGBA){let j=ae?cr:Ve.getTransfer(Z);U===n.FLOAT&&(J=n.RGBA32F),U===n.HALF_FLOAT&&(J=n.RGBA16F),U===n.UNSIGNED_BYTE&&(J=j===nt?n.SRGB8_ALPHA8:n.RGBA8),U===n.UNSIGNED_SHORT&&le&&(J=le.RGBA16_EXT),U===n.SHORT&&le&&(J=le.RGBA16_SNORM_EXT),U===n.UNSIGNED_SHORT_4_4_4_4&&(J=n.RGBA4),U===n.UNSIGNED_SHORT_5_5_5_1&&(J=n.RGB5_A1)}return(J===n.R16F||J===n.R32F||J===n.RG16F||J===n.RG32F||J===n.RGBA16F||J===n.RGBA32F)&&e.get("EXT_color_buffer_float"),J}function v(T,x){let U;return T?x===null||x===bn||x===Es?U=n.DEPTH24_STENCIL8:x===Dn?U=n.DEPTH32F_STENCIL8:x===As&&(U=n.DEPTH24_STENCIL8,pe("DepthTexture: 16 bit depth attachment is not supported with stencil. Using 24-bit attachment.")):x===null||x===bn||x===Es?U=n.DEPTH_COMPONENT24:x===Dn?U=n.DEPTH_COMPONENT32F:x===As&&(U=n.DEPTH_COMPONENT16),U}function A(T,x){return p(T)===!0||T.isFramebufferTexture&&T.minFilter!==Rt&&T.minFilter!==Bt?Math.log2(Math.max(x.width,x.height))+1:T.mipmaps!==void 0&&T.mipmaps.length>0?T.mipmaps.length:T.isCompressedTexture&&Array.isArray(T.image)?x.mipmaps.length:1}function R(T){let x=T.target;x.removeEventListener("dispose",R),M(x),x.isVideoTexture&&u.delete(x),x.isHTMLTexture&&d.delete(x)}function b(T){let x=T.target;x.removeEventListener("dispose",b),D(x)}function M(T){let x=i.get(T);if(x.__webglInit===void 0)return;let U=T.source,z=f.get(U);if(z){let Z=z[x.__cacheKey];Z.usedTimes--,Z.usedTimes===0&&B(T),Object.keys(z).length===0&&f.delete(U)}i.remove(T)}function B(T){let x=i.get(T);n.deleteTexture(x.__webglTexture);let U=T.source,z=f.get(U);delete z[x.__cacheKey],o.memory.textures--}function D(T){let x=i.get(T);if(T.depthTexture&&(T.depthTexture.dispose(),i.remove(T.depthTexture)),T.isWebGLCubeRenderTarget)for(let z=0;z<6;z++){if(Array.isArray(x.__webglFramebuffer[z]))for(let Z=0;Z<x.__webglFramebuffer[z].length;Z++)n.deleteFramebuffer(x.__webglFramebuffer[z][Z]);else n.deleteFramebuffer(x.__webglFramebuffer[z]);x.__webglDepthbuffer&&n.deleteRenderbuffer(x.__webglDepthbuffer[z])}else{if(Array.isArray(x.__webglFramebuffer))for(let z=0;z<x.__webglFramebuffer.length;z++)n.deleteFramebuffer(x.__webglFramebuffer[z]);else n.deleteFramebuffer(x.__webglFramebuffer);if(x.__webglDepthbuffer&&n.deleteRenderbuffer(x.__webglDepthbuffer),x.__webglMultisampledFramebuffer&&n.deleteFramebuffer(x.__webglMultisampledFramebuffer),x.__webglColorRenderbuffer)for(let z=0;z<x.__webglColorRenderbuffer.length;z++)x.__webglColorRenderbuffer[z]&&n.deleteRenderbuffer(x.__webglColorRenderbuffer[z]);x.__webglDepthRenderbuffer&&n.deleteRenderbuffer(x.__webglDepthRenderbuffer)}let U=T.textures;for(let z=0,Z=U.length;z<Z;z++){let ae=i.get(U[z]);ae.__webglTexture&&(n.deleteTexture(ae.__webglTexture),o.memory.textures--),i.remove(U[z])}i.remove(T)}let k=0;function H(){k=0}function F(){return k}function O(T){k=T}function q(){let T=k;return T>=s.maxTextures&&pe("WebGLTextures: Trying to use "+(T+1)+" texture units while this GPU supports only "+s.maxTextures),k+=1,T}function X(T){let x=[];return x.push(T.wrapS),x.push(T.wrapT),x.push(T.wrapR||0),x.push(T.magFilter),x.push(T.minFilter),x.push(T.anisotropy),x.push(T.internalFormat),x.push(T.format),x.push(T.type),x.push(T.generateMipmaps),x.push(T.premultiplyAlpha),x.push(T.flipY),x.push(T.unpackAlignment),x.push(T.colorSpace),x.join()}function re(T,x){let U=i.get(T);if(T.isVideoTexture&&I(T),T.isRenderTargetTexture===!1&&T.isExternalTexture!==!0&&T.version>0&&U.__version!==T.version){let z=T.image;if(z===null)pe("WebGLRenderer: Texture marked for update but no image data found.");else if(z.complete===!1)pe("WebGLRenderer: Texture marked for update but image is incomplete");else{Te(U,T,x);return}}else T.isExternalTexture&&(U.__webglTexture=T.sourceTexture?T.sourceTexture:null);t.bindTexture(n.TEXTURE_2D,U.__webglTexture,n.TEXTURE0+x)}function K(T,x){let U=i.get(T);if(T.isRenderTargetTexture===!1&&T.version>0&&U.__version!==T.version){Te(U,T,x);return}else T.isExternalTexture&&(U.__webglTexture=T.sourceTexture?T.sourceTexture:null);t.bindTexture(n.TEXTURE_2D_ARRAY,U.__webglTexture,n.TEXTURE0+x)}function ne(T,x){let U=i.get(T);if(T.isRenderTargetTexture===!1&&T.version>0&&U.__version!==T.version){Te(U,T,x);return}t.bindTexture(n.TEXTURE_3D,U.__webglTexture,n.TEXTURE0+x)}function se(T,x){let U=i.get(T);if(T.isCubeDepthTexture!==!0&&T.version>0&&U.__version!==T.version){Oe(U,T,x);return}t.bindTexture(n.TEXTURE_CUBE_MAP,U.__webglTexture,n.TEXTURE0+x)}let Ue={[ar]:n.REPEAT,[Ln]:n.CLAMP_TO_EDGE,[lr]:n.MIRRORED_REPEAT},Fe={[Rt]:n.NEAREST,[Eh]:n.NEAREST_MIPMAP_NEAREST,[Mr]:n.NEAREST_MIPMAP_LINEAR,[Bt]:n.LINEAR,[Io]:n.LINEAR_MIPMAP_NEAREST,[di]:n.LINEAR_MIPMAP_LINEAR},yt={[lc]:n.NEVER,[hc]:n.ALWAYS,[ma]:n.LESS,[ws]:n.LEQUAL,[cc]:n.EQUAL,[$i]:n.GEQUAL,[Fr]:n.GREATER,[uc]:n.NOTEQUAL};function je(T,x){if(x.type===Dn&&e.has("OES_texture_float_linear")===!1&&(x.magFilter===Bt||x.magFilter===Io||x.magFilter===Mr||x.magFilter===di||x.minFilter===Bt||x.minFilter===Io||x.minFilter===Mr||x.minFilter===di)&&pe("WebGLRenderer: Unable to use linear filtering with floating point textures. OES_texture_float_linear not supported on this device."),n.texParameteri(T,n.TEXTURE_WRAP_S,Ue[x.wrapS]),n.texParameteri(T,n.TEXTURE_WRAP_T,Ue[x.wrapT]),(T===n.TEXTURE_3D||T===n.TEXTURE_2D_ARRAY)&&n.texParameteri(T,n.TEXTURE_WRAP_R,Ue[x.wrapR]),n.texParameteri(T,n.TEXTURE_MAG_FILTER,Fe[x.magFilter]),n.texParameteri(T,n.TEXTURE_MIN_FILTER,Fe[x.minFilter]),x.compareFunction&&(n.texParameteri(T,n.TEXTURE_COMPARE_MODE,n.COMPARE_REF_TO_TEXTURE),n.texParameteri(T,n.TEXTURE_COMPARE_FUNC,yt[x.compareFunction])),e.has("EXT_texture_filter_anisotropic")===!0){if(x.magFilter===Rt||x.minFilter!==Mr&&x.minFilter!==di||x.type===Dn&&e.has("OES_texture_float_linear")===!1)return;if(x.anisotropy>1||i.get(x).__currentAnisotropy){let U=e.get("EXT_texture_filter_anisotropic");n.texParameterf(T,U.TEXTURE_MAX_ANISOTROPY_EXT,Math.min(x.anisotropy,s.getMaxAnisotropy())),i.get(x).__currentAnisotropy=x.anisotropy}}}function rt(T,x){let U=!1;T.__webglInit===void 0&&(T.__webglInit=!0,x.addEventListener("dispose",R));let z=x.source,Z=f.get(z);Z===void 0&&(Z={},f.set(z,Z));let ae=X(x);if(ae!==T.__cacheKey){Z[ae]===void 0&&(Z[ae]={texture:n.createTexture(),usedTimes:0},o.memory.textures++,U=!0),Z[ae].usedTimes++;let le=Z[T.__cacheKey];le!==void 0&&(Z[T.__cacheKey].usedTimes--,le.usedTimes===0&&B(x)),T.__cacheKey=ae,T.__webglTexture=Z[ae].texture}return U}function $(T,x,U){return Math.floor(Math.floor(T/U)/x)}function te(T,x,U,z){let ae=T.updateRanges;if(ae.length===0)t.texSubImage2D(n.TEXTURE_2D,0,0,0,x.width,x.height,U,z,x.data);else{ae.sort((Ie,fe)=>Ie.start-fe.start);let le=0;for(let Ie=1;Ie<ae.length;Ie++){let fe=ae[le],ue=ae[Ie],Be=fe.start+fe.count,Pe=$(ue.start,x.width,4),We=$(fe.start,x.width,4);ue.start<=Be+1&&Pe===We&&$(ue.start+ue.count-1,x.width,4)===Pe?fe.count=Math.max(fe.count,ue.start+ue.count-fe.start):(++le,ae[le]=ue)}ae.length=le+1;let J=t.getParameter(n.UNPACK_ROW_LENGTH),j=t.getParameter(n.UNPACK_SKIP_PIXELS),ce=t.getParameter(n.UNPACK_SKIP_ROWS);t.pixelStorei(n.UNPACK_ROW_LENGTH,x.width);for(let Ie=0,fe=ae.length;Ie<fe;Ie++){let ue=ae[Ie],Be=Math.floor(ue.start/4),Pe=Math.ceil(ue.count/4),We=Be%x.width,N=Math.floor(Be/x.width),he=Pe,Q=1;t.pixelStorei(n.UNPACK_SKIP_PIXELS,We),t.pixelStorei(n.UNPACK_SKIP_ROWS,N),t.texSubImage2D(n.TEXTURE_2D,0,We,N,he,Q,U,z,x.data)}T.clearUpdateRanges(),t.pixelStorei(n.UNPACK_ROW_LENGTH,J),t.pixelStorei(n.UNPACK_SKIP_PIXELS,j),t.pixelStorei(n.UNPACK_SKIP_ROWS,ce)}}function Te(T,x,U){let z=n.TEXTURE_2D;(x.isDataArrayTexture||x.isCompressedArrayTexture)&&(z=n.TEXTURE_2D_ARRAY),x.isData3DTexture&&(z=n.TEXTURE_3D);let Z=rt(T,x),ae=x.source;t.bindTexture(z,T.__webglTexture,n.TEXTURE0+U);let le=i.get(ae);if(ae.version!==le.__version||Z===!0){if(t.activeTexture(n.TEXTURE0+U),(typeof ImageBitmap<"u"&&x.image instanceof ImageBitmap)===!1){let Q=Ve.getPrimaries(Ve.workingColorSpace),de=x.colorSpace===Pn?null:Ve.getPrimaries(x.colorSpace),be=x.colorSpace===Pn||Q===de?n.NONE:n.BROWSER_DEFAULT_WEBGL;t.pixelStorei(n.UNPACK_FLIP_Y_WEBGL,x.flipY),t.pixelStorei(n.UNPACK_PREMULTIPLY_ALPHA_WEBGL,x.premultiplyAlpha),t.pixelStorei(n.UNPACK_COLORSPACE_CONVERSION_WEBGL,be)}t.pixelStorei(n.UNPACK_ALIGNMENT,x.unpackAlignment);let j=m(x.image,!1,s.maxTextureSize);j=Qt(x,j);let ce=r.convert(x.format,x.colorSpace),Ie=r.convert(x.type),fe=S(x.internalFormat,ce,Ie,x.normalized,x.colorSpace,x.isVideoTexture);je(z,x);let ue,Be=x.mipmaps,Pe=x.isVideoTexture!==!0,We=le.__version===void 0||Z===!0,N=ae.dataReady,he=A(x,j);if(x.isDepthTexture)fe=v(x.format===fi,x.type),We&&(Pe?t.texStorage2D(n.TEXTURE_2D,1,fe,j.width,j.height):t.texImage2D(n.TEXTURE_2D,0,fe,j.width,j.height,0,ce,Ie,null));else if(x.isDataTexture)if(Be.length>0){Pe&&We&&t.texStorage2D(n.TEXTURE_2D,he,fe,Be[0].width,Be[0].height);for(let Q=0,de=Be.length;Q<de;Q++)ue=Be[Q],Pe?N&&t.texSubImage2D(n.TEXTURE_2D,Q,0,0,ue.width,ue.height,ce,Ie,ue.data):t.texImage2D(n.TEXTURE_2D,Q,fe,ue.width,ue.height,0,ce,Ie,ue.data);x.generateMipmaps=!1}else Pe?(We&&t.texStorage2D(n.TEXTURE_2D,he,fe,j.width,j.height),N&&te(x,j,ce,Ie)):t.texImage2D(n.TEXTURE_2D,0,fe,j.width,j.height,0,ce,Ie,j.data);else if(x.isCompressedTexture)if(x.isCompressedArrayTexture){Pe&&We&&t.texStorage3D(n.TEXTURE_2D_ARRAY,he,fe,Be[0].width,Be[0].height,j.depth);for(let Q=0,de=Be.length;Q<de;Q++)if(ue=Be[Q],x.format!==gn)if(ce!==null)if(Pe){if(N)if(x.layerUpdates.size>0){let be=dc(ue.width,ue.height,x.format,x.type);for(let ie of x.layerUpdates){let Le=ue.data.subarray(ie*be/ue.data.BYTES_PER_ELEMENT,(ie+1)*be/ue.data.BYTES_PER_ELEMENT);t.compressedTexSubImage3D(n.TEXTURE_2D_ARRAY,Q,0,0,ie,ue.width,ue.height,1,ce,Le)}}else t.compressedTexSubImage3D(n.TEXTURE_2D_ARRAY,Q,0,0,0,ue.width,ue.height,j.depth,ce,ue.data)}else t.compressedTexImage3D(n.TEXTURE_2D_ARRAY,Q,fe,ue.width,ue.height,j.depth,0,ue.data,0,0);else pe("WebGLRenderer: Attempt to load unsupported compressed texture format in .uploadTexture()");else Pe?N&&t.texSubImage3D(n.TEXTURE_2D_ARRAY,Q,0,0,0,ue.width,ue.height,j.depth,ce,Ie,ue.data):t.texImage3D(n.TEXTURE_2D_ARRAY,Q,fe,ue.width,ue.height,j.depth,0,ce,Ie,ue.data);x.layerUpdates.size>0&&x.clearLayerUpdates()}else{Pe&&We&&t.texStorage2D(n.TEXTURE_2D,he,fe,Be[0].width,Be[0].height);for(let Q=0,de=Be.length;Q<de;Q++)ue=Be[Q],x.format!==gn?ce!==null?Pe?N&&t.compressedTexSubImage2D(n.TEXTURE_2D,Q,0,0,ue.width,ue.height,ce,ue.data):t.compressedTexImage2D(n.TEXTURE_2D,Q,fe,ue.width,ue.height,0,ue.data):pe("WebGLRenderer: Attempt to load unsupported compressed texture format in .uploadTexture()"):Pe?N&&t.texSubImage2D(n.TEXTURE_2D,Q,0,0,ue.width,ue.height,ce,Ie,ue.data):t.texImage2D(n.TEXTURE_2D,Q,fe,ue.width,ue.height,0,ce,Ie,ue.data)}else if(x.isDataArrayTexture)if(Pe){if(We&&t.texStorage3D(n.TEXTURE_2D_ARRAY,he,fe,j.width,j.height,j.depth),N)if(x.layerUpdates.size>0){let Q=dc(j.width,j.height,x.format,x.type);for(let de of x.layerUpdates){let be=j.data.subarray(de*Q/j.data.BYTES_PER_ELEMENT,(de+1)*Q/j.data.BYTES_PER_ELEMENT);t.texSubImage3D(n.TEXTURE_2D_ARRAY,0,0,0,de,j.width,j.height,1,ce,Ie,be)}x.clearLayerUpdates()}else t.texSubImage3D(n.TEXTURE_2D_ARRAY,0,0,0,0,j.width,j.height,j.depth,ce,Ie,j.data)}else t.texImage3D(n.TEXTURE_2D_ARRAY,0,fe,j.width,j.height,j.depth,0,ce,Ie,j.data);else if(x.isData3DTexture)Pe?(We&&t.texStorage3D(n.TEXTURE_3D,he,fe,j.width,j.height,j.depth),N&&t.texSubImage3D(n.TEXTURE_3D,0,0,0,0,j.width,j.height,j.depth,ce,Ie,j.data)):t.texImage3D(n.TEXTURE_3D,0,fe,j.width,j.height,j.depth,0,ce,Ie,j.data);else if(x.isFramebufferTexture){if(We)if(Pe)t.texStorage2D(n.TEXTURE_2D,he,fe,j.width,j.height);else{let Q=j.width,de=j.height;for(let be=0;be<he;be++)t.texImage2D(n.TEXTURE_2D,be,fe,Q,de,0,ce,Ie,null),Q>>=1,de>>=1}}else if(x.isHTMLTexture){if("texElementImage2D"in n){let Q=n.canvas;if(Q.hasAttribute("layoutsubtree")||Q.setAttribute("layoutsubtree","true"),j.parentNode!==Q){Q.appendChild(j),d.add(x),Q.onpaint=de=>{let be=de.changedElements;for(let ie of d)be.includes(ie.image)&&(ie.needsUpdate=!0)},Q.requestPaint();return}if(n.texElementImage2D.length===3)n.texElementImage2D(n.TEXTURE_2D,n.RGBA8,j);else{let be=n.RGBA,ie=n.RGBA,Le=n.UNSIGNED_BYTE;n.texElementImage2D(n.TEXTURE_2D,0,be,ie,Le,j)}n.texParameteri(n.TEXTURE_2D,n.TEXTURE_MIN_FILTER,n.LINEAR),n.texParameteri(n.TEXTURE_2D,n.TEXTURE_WRAP_S,n.CLAMP_TO_EDGE),n.texParameteri(n.TEXTURE_2D,n.TEXTURE_WRAP_T,n.CLAMP_TO_EDGE)}}else if(Be.length>0){if(Pe&&We){let Q=ut(Be[0]);t.texStorage2D(n.TEXTURE_2D,he,fe,Q.width,Q.height)}for(let Q=0,de=Be.length;Q<de;Q++)ue=Be[Q],Pe?N&&t.texSubImage2D(n.TEXTURE_2D,Q,0,0,ce,Ie,ue):t.texImage2D(n.TEXTURE_2D,Q,fe,ce,Ie,ue);x.generateMipmaps=!1}else if(Pe){if(We){let Q=ut(j);t.texStorage2D(n.TEXTURE_2D,he,fe,Q.width,Q.height)}N&&t.texSubImage2D(n.TEXTURE_2D,0,0,0,ce,Ie,j)}else t.texImage2D(n.TEXTURE_2D,0,fe,ce,Ie,j);p(x)&&E(z),le.__version=ae.version,x.onUpdate&&x.onUpdate(x)}T.__version=x.version}function Oe(T,x,U){if(x.image.length!==6)return;let z=rt(T,x),Z=x.source;t.bindTexture(n.TEXTURE_CUBE_MAP,T.__webglTexture,n.TEXTURE0+U);let ae=i.get(Z);if(Z.version!==ae.__version||z===!0){t.activeTexture(n.TEXTURE0+U);let le=Ve.getPrimaries(Ve.workingColorSpace),J=x.colorSpace===Pn?null:Ve.getPrimaries(x.colorSpace),j=x.colorSpace===Pn||le===J?n.NONE:n.BROWSER_DEFAULT_WEBGL;t.pixelStorei(n.UNPACK_FLIP_Y_WEBGL,x.flipY),t.pixelStorei(n.UNPACK_PREMULTIPLY_ALPHA_WEBGL,x.premultiplyAlpha),t.pixelStorei(n.UNPACK_ALIGNMENT,x.unpackAlignment),t.pixelStorei(n.UNPACK_COLORSPACE_CONVERSION_WEBGL,j);let ce=x.isCompressedTexture||x.image[0].isCompressedTexture,Ie=x.image[0]&&x.image[0].isDataTexture,fe=[];for(let ie=0;ie<6;ie++)!ce&&!Ie?fe[ie]=m(x.image[ie],!0,s.maxCubemapSize):fe[ie]=Ie?x.image[ie].image:x.image[ie],fe[ie]=Qt(x,fe[ie]);let ue=fe[0],Be=r.convert(x.format,x.colorSpace),Pe=r.convert(x.type),We=S(x.internalFormat,Be,Pe,x.normalized,x.colorSpace),N=x.isVideoTexture!==!0,he=ae.__version===void 0||z===!0,Q=Z.dataReady,de=A(x,ue);je(n.TEXTURE_CUBE_MAP,x);let be;if(ce){N&&he&&t.texStorage2D(n.TEXTURE_CUBE_MAP,de,We,ue.width,ue.height);for(let ie=0;ie<6;ie++){be=fe[ie].mipmaps;for(let Le=0;Le<be.length;Le++){let Re=be[Le];x.format!==gn?Be!==null?N?Q&&t.compressedTexSubImage2D(n.TEXTURE_CUBE_MAP_POSITIVE_X+ie,Le,0,0,Re.width,Re.height,Be,Re.data):t.compressedTexImage2D(n.TEXTURE_CUBE_MAP_POSITIVE_X+ie,Le,We,Re.width,Re.height,0,Re.data):pe("WebGLRenderer: Attempt to load unsupported compressed texture format in .setTextureCube()"):N?Q&&t.texSubImage2D(n.TEXTURE_CUBE_MAP_POSITIVE_X+ie,Le,0,0,Re.width,Re.height,Be,Pe,Re.data):t.texImage2D(n.TEXTURE_CUBE_MAP_POSITIVE_X+ie,Le,We,Re.width,Re.height,0,Be,Pe,Re.data)}}}else{if(be=x.mipmaps,N&&he){be.length>0&&de++;let ie=ut(fe[0]);t.texStorage2D(n.TEXTURE_CUBE_MAP,de,We,ie.width,ie.height)}for(let ie=0;ie<6;ie++)if(Ie){N?Q&&t.texSubImage2D(n.TEXTURE_CUBE_MAP_POSITIVE_X+ie,0,0,0,fe[ie].width,fe[ie].height,Be,Pe,fe[ie].data):t.texImage2D(n.TEXTURE_CUBE_MAP_POSITIVE_X+ie,0,We,fe[ie].width,fe[ie].height,0,Be,Pe,fe[ie].data);for(let Le=0;Le<be.length;Le++){let bt=be[Le].image[ie].image;N?Q&&t.texSubImage2D(n.TEXTURE_CUBE_MAP_POSITIVE_X+ie,Le+1,0,0,bt.width,bt.height,Be,Pe,bt.data):t.texImage2D(n.TEXTURE_CUBE_MAP_POSITIVE_X+ie,Le+1,We,bt.width,bt.height,0,Be,Pe,bt.data)}}else{N?Q&&t.texSubImage2D(n.TEXTURE_CUBE_MAP_POSITIVE_X+ie,0,0,0,Be,Pe,fe[ie]):t.texImage2D(n.TEXTURE_CUBE_MAP_POSITIVE_X+ie,0,We,Be,Pe,fe[ie]);for(let Le=0;Le<be.length;Le++){let Re=be[Le];N?Q&&t.texSubImage2D(n.TEXTURE_CUBE_MAP_POSITIVE_X+ie,Le+1,0,0,Be,Pe,Re.image[ie]):t.texImage2D(n.TEXTURE_CUBE_MAP_POSITIVE_X+ie,Le+1,We,Be,Pe,Re.image[ie])}}}p(x)&&E(n.TEXTURE_CUBE_MAP),ae.__version=Z.version,x.onUpdate&&x.onUpdate(x)}T.__version=x.version}function ve(T,x,U,z,Z,ae){let le=r.convert(U.format,U.colorSpace),J=r.convert(U.type),j=S(U.internalFormat,le,J,U.normalized,U.colorSpace),ce=i.get(x),Ie=i.get(U);if(Ie.__renderTarget=x,!ce.__hasExternalTextures){let fe=Math.max(1,x.width>>ae),ue=Math.max(1,x.height>>ae);Z===n.TEXTURE_3D||Z===n.TEXTURE_2D_ARRAY?t.texImage3D(Z,ae,j,fe,ue,x.depth,0,le,J,null):t.texImage2D(Z,ae,j,fe,ue,0,le,J,null)}t.bindFramebuffer(n.FRAMEBUFFER,T),Ft(x)?a.framebufferTexture2DMultisampleEXT(n.FRAMEBUFFER,z,Z,Ie.__webglTexture,0,Mt(x)):(Z===n.TEXTURE_2D||Z>=n.TEXTURE_CUBE_MAP_POSITIVE_X&&Z<=n.TEXTURE_CUBE_MAP_NEGATIVE_Z)&&n.framebufferTexture2D(n.FRAMEBUFFER,z,Z,Ie.__webglTexture,ae),t.bindFramebuffer(n.FRAMEBUFFER,null)}function Ke(T,x,U){if(n.bindRenderbuffer(n.RENDERBUFFER,T),x.depthBuffer){let z=x.depthTexture,Z=z&&z.isDepthTexture?z.type:null,ae=v(x.stencilBuffer,Z),le=x.stencilBuffer?n.DEPTH_STENCIL_ATTACHMENT:n.DEPTH_ATTACHMENT;Ft(x)?a.renderbufferStorageMultisampleEXT(n.RENDERBUFFER,Mt(x),ae,x.width,x.height):U?n.renderbufferStorageMultisample(n.RENDERBUFFER,Mt(x),ae,x.width,x.height):n.renderbufferStorage(n.RENDERBUFFER,ae,x.width,x.height),n.framebufferRenderbuffer(n.FRAMEBUFFER,le,n.RENDERBUFFER,T)}else{let z=x.textures;for(let Z=0;Z<z.length;Z++){let ae=z[Z],le=r.convert(ae.format,ae.colorSpace),J=r.convert(ae.type),j=S(ae.internalFormat,le,J,ae.normalized,ae.colorSpace);Ft(x)?a.renderbufferStorageMultisampleEXT(n.RENDERBUFFER,Mt(x),j,x.width,x.height):U?n.renderbufferStorageMultisample(n.RENDERBUFFER,Mt(x),j,x.width,x.height):n.renderbufferStorage(n.RENDERBUFFER,j,x.width,x.height)}}n.bindRenderbuffer(n.RENDERBUFFER,null)}function Ot(T,x,U){let z=x.isWebGLCubeRenderTarget===!0;if(t.bindFramebuffer(n.FRAMEBUFFER,T),!(x.depthTexture&&x.depthTexture.isDepthTexture))throw new Error("THREE.WebGLTextures: renderTarget.depthTexture must be an instance of THREE.DepthTexture.");let Z=i.get(x.depthTexture);if(Z.__renderTarget=x,(!Z.__webglTexture||x.depthTexture.image.width!==x.width||x.depthTexture.image.height!==x.height)&&(x.depthTexture.image.width=x.width,x.depthTexture.image.height=x.height,x.depthTexture.needsUpdate=!0),z){if(Z.__webglInit===void 0&&(Z.__webglInit=!0,x.depthTexture.addEventListener("dispose",R)),Z.__webglTexture===void 0){Z.__webglTexture=n.createTexture(),t.bindTexture(n.TEXTURE_CUBE_MAP,Z.__webglTexture),je(n.TEXTURE_CUBE_MAP,x.depthTexture);let ce=r.convert(x.depthTexture.format),Ie=r.convert(x.depthTexture.type),fe;x.depthTexture.format===Un?fe=n.DEPTH_COMPONENT24:x.depthTexture.format===fi&&(fe=n.DEPTH24_STENCIL8);for(let ue=0;ue<6;ue++)n.texImage2D(n.TEXTURE_CUBE_MAP_POSITIVE_X+ue,0,fe,x.width,x.height,0,ce,Ie,null)}}else re(x.depthTexture,0);let ae=Z.__webglTexture,le=Mt(x),J=z?n.TEXTURE_CUBE_MAP_POSITIVE_X+U:n.TEXTURE_2D,j=x.depthTexture.format===fi?n.DEPTH_STENCIL_ATTACHMENT:n.DEPTH_ATTACHMENT;if(x.depthTexture.format===Un)Ft(x)?a.framebufferTexture2DMultisampleEXT(n.FRAMEBUFFER,j,J,ae,0,le):n.framebufferTexture2D(n.FRAMEBUFFER,j,J,ae,0);else if(x.depthTexture.format===fi)Ft(x)?a.framebufferTexture2DMultisampleEXT(n.FRAMEBUFFER,j,J,ae,0,le):n.framebufferTexture2D(n.FRAMEBUFFER,j,J,ae,0);else throw new Error("THREE.WebGLTextures: Unknown depthTexture format.")}function $e(T){let x=i.get(T),U=T.isWebGLCubeRenderTarget===!0;if(x.__boundDepthTexture!==T.depthTexture){let z=T.depthTexture;if(x.__depthDisposeCallback&&x.__depthDisposeCallback(),z){let Z=()=>{delete x.__boundDepthTexture,delete x.__depthDisposeCallback,z.removeEventListener("dispose",Z)};z.addEventListener("dispose",Z),x.__depthDisposeCallback=Z}x.__boundDepthTexture=z}if(T.depthTexture&&!x.__autoAllocateDepthBuffer)if(U)for(let z=0;z<6;z++)Ot(x.__webglFramebuffer[z],T,z);else{let z=T.texture.mipmaps;z&&z.length>0?Ot(x.__webglFramebuffer[0],T,0):Ot(x.__webglFramebuffer,T,0)}else if(U){x.__webglDepthbuffer=[];for(let z=0;z<6;z++)if(t.bindFramebuffer(n.FRAMEBUFFER,x.__webglFramebuffer[z]),x.__webglDepthbuffer[z]===void 0)x.__webglDepthbuffer[z]=n.createRenderbuffer(),Ke(x.__webglDepthbuffer[z],T,!1);else{let Z=T.stencilBuffer?n.DEPTH_STENCIL_ATTACHMENT:n.DEPTH_ATTACHMENT,ae=x.__webglDepthbuffer[z];n.bindRenderbuffer(n.RENDERBUFFER,ae),n.framebufferRenderbuffer(n.FRAMEBUFFER,Z,n.RENDERBUFFER,ae)}}else{let z=T.texture.mipmaps;if(z&&z.length>0?t.bindFramebuffer(n.FRAMEBUFFER,x.__webglFramebuffer[0]):t.bindFramebuffer(n.FRAMEBUFFER,x.__webglFramebuffer),x.__webglDepthbuffer===void 0)x.__webglDepthbuffer=n.createRenderbuffer(),Ke(x.__webglDepthbuffer,T,!1);else{let Z=T.stencilBuffer?n.DEPTH_STENCIL_ATTACHMENT:n.DEPTH_ATTACHMENT,ae=x.__webglDepthbuffer;n.bindRenderbuffer(n.RENDERBUFFER,ae),n.framebufferRenderbuffer(n.FRAMEBUFFER,Z,n.RENDERBUFFER,ae)}}t.bindFramebuffer(n.FRAMEBUFFER,null)}function st(T,x,U){let z=i.get(T);x!==void 0&&ve(z.__webglFramebuffer,T,T.texture,n.COLOR_ATTACHMENT0,n.TEXTURE_2D,0),U!==void 0&&$e(T)}function _t(T){let x=T.texture,U=i.get(T),z=i.get(x);T.addEventListener("dispose",b);let Z=T.textures,ae=T.isWebGLCubeRenderTarget===!0,le=Z.length>1;if(le||(z.__webglTexture===void 0&&(z.__webglTexture=n.createTexture()),z.__version=x.version,o.memory.textures++),ae){U.__webglFramebuffer=[];for(let J=0;J<6;J++)if(x.mipmaps&&x.mipmaps.length>0){U.__webglFramebuffer[J]=[];for(let j=0;j<x.mipmaps.length;j++)U.__webglFramebuffer[J][j]=n.createFramebuffer()}else U.__webglFramebuffer[J]=n.createFramebuffer()}else{if(x.mipmaps&&x.mipmaps.length>0){U.__webglFramebuffer=[];for(let J=0;J<x.mipmaps.length;J++)U.__webglFramebuffer[J]=n.createFramebuffer()}else U.__webglFramebuffer=n.createFramebuffer();if(le)for(let J=0,j=Z.length;J<j;J++){let ce=i.get(Z[J]);ce.__webglTexture===void 0&&(ce.__webglTexture=n.createTexture(),o.memory.textures++)}if(T.samples>0&&Ft(T)===!1){U.__webglMultisampledFramebuffer=n.createFramebuffer(),U.__webglColorRenderbuffer=[],t.bindFramebuffer(n.FRAMEBUFFER,U.__webglMultisampledFramebuffer);for(let J=0;J<Z.length;J++){let j=Z[J];U.__webglColorRenderbuffer[J]=n.createRenderbuffer(),n.bindRenderbuffer(n.RENDERBUFFER,U.__webglColorRenderbuffer[J]);let ce=r.convert(j.format,j.colorSpace),Ie=r.convert(j.type),fe=S(j.internalFormat,ce,Ie,j.normalized,j.colorSpace,T.isXRRenderTarget===!0),ue=Mt(T);n.renderbufferStorageMultisample(n.RENDERBUFFER,ue,fe,T.width,T.height),n.framebufferRenderbuffer(n.FRAMEBUFFER,n.COLOR_ATTACHMENT0+J,n.RENDERBUFFER,U.__webglColorRenderbuffer[J])}n.bindRenderbuffer(n.RENDERBUFFER,null),T.depthBuffer&&(U.__webglDepthRenderbuffer=n.createRenderbuffer(),Ke(U.__webglDepthRenderbuffer,T,!0)),t.bindFramebuffer(n.FRAMEBUFFER,null)}}if(ae){t.bindTexture(n.TEXTURE_CUBE_MAP,z.__webglTexture),je(n.TEXTURE_CUBE_MAP,x);for(let J=0;J<6;J++)if(x.mipmaps&&x.mipmaps.length>0)for(let j=0;j<x.mipmaps.length;j++)ve(U.__webglFramebuffer[J][j],T,x,n.COLOR_ATTACHMENT0,n.TEXTURE_CUBE_MAP_POSITIVE_X+J,j);else ve(U.__webglFramebuffer[J],T,x,n.COLOR_ATTACHMENT0,n.TEXTURE_CUBE_MAP_POSITIVE_X+J,0);p(x)&&E(n.TEXTURE_CUBE_MAP),t.unbindTexture()}else if(le){for(let J=0,j=Z.length;J<j;J++){let ce=Z[J],Ie=i.get(ce),fe=n.TEXTURE_2D;(T.isWebGL3DRenderTarget||T.isWebGLArrayRenderTarget)&&(fe=T.isWebGL3DRenderTarget?n.TEXTURE_3D:n.TEXTURE_2D_ARRAY),t.bindTexture(fe,Ie.__webglTexture),je(fe,ce),ve(U.__webglFramebuffer,T,ce,n.COLOR_ATTACHMENT0+J,fe,0),p(ce)&&E(fe)}t.unbindTexture()}else{let J=n.TEXTURE_2D;if((T.isWebGL3DRenderTarget||T.isWebGLArrayRenderTarget)&&(J=T.isWebGL3DRenderTarget?n.TEXTURE_3D:n.TEXTURE_2D_ARRAY),t.bindTexture(J,z.__webglTexture),je(J,x),x.mipmaps&&x.mipmaps.length>0)for(let j=0;j<x.mipmaps.length;j++)ve(U.__webglFramebuffer[j],T,x,n.COLOR_ATTACHMENT0,J,j);else ve(U.__webglFramebuffer,T,x,n.COLOR_ATTACHMENT0,J,0);p(x)&&E(J),t.unbindTexture()}T.depthBuffer&&$e(T)}function Qe(T){let x=T.textures;for(let U=0,z=x.length;U<z;U++){let Z=x[U];if(p(Z)){let ae=w(T),le=i.get(Z).__webglTexture;t.bindTexture(ae,le),E(ae),t.unbindTexture()}}}let Tt=[],Yt=[];function xn(T){if(T.samples>0){if(Ft(T)===!1){let x=T.textures,U=T.width,z=T.height,Z=n.COLOR_BUFFER_BIT,ae=T.stencilBuffer?n.DEPTH_STENCIL_ATTACHMENT:n.DEPTH_ATTACHMENT,le=i.get(T),J=x.length>1;if(J)for(let ce=0;ce<x.length;ce++)t.bindFramebuffer(n.FRAMEBUFFER,le.__webglMultisampledFramebuffer),n.framebufferRenderbuffer(n.FRAMEBUFFER,n.COLOR_ATTACHMENT0+ce,n.RENDERBUFFER,null),t.bindFramebuffer(n.FRAMEBUFFER,le.__webglFramebuffer),n.framebufferTexture2D(n.DRAW_FRAMEBUFFER,n.COLOR_ATTACHMENT0+ce,n.TEXTURE_2D,null,0);t.bindFramebuffer(n.READ_FRAMEBUFFER,le.__webglMultisampledFramebuffer);let j=T.texture.mipmaps;j&&j.length>0?t.bindFramebuffer(n.DRAW_FRAMEBUFFER,le.__webglFramebuffer[0]):t.bindFramebuffer(n.DRAW_FRAMEBUFFER,le.__webglFramebuffer);for(let ce=0;ce<x.length;ce++){if(T.resolveDepthBuffer&&(T.depthBuffer&&(Z|=n.DEPTH_BUFFER_BIT),T.stencilBuffer&&T.resolveStencilBuffer&&(Z|=n.STENCIL_BUFFER_BIT)),J){n.framebufferRenderbuffer(n.READ_FRAMEBUFFER,n.COLOR_ATTACHMENT0,n.RENDERBUFFER,le.__webglColorRenderbuffer[ce]);let Ie=i.get(x[ce]).__webglTexture;n.framebufferTexture2D(n.DRAW_FRAMEBUFFER,n.COLOR_ATTACHMENT0,n.TEXTURE_2D,Ie,0)}n.blitFramebuffer(0,0,U,z,0,0,U,z,Z,n.NEAREST),l===!0&&(Tt.length=0,Yt.length=0,Tt.push(n.COLOR_ATTACHMENT0+ce),T.depthBuffer&&T.storeMultisampledDepthBuffer===!1&&(Tt.push(ae),Yt.push(ae),n.invalidateFramebuffer(n.DRAW_FRAMEBUFFER,Yt)),n.invalidateFramebuffer(n.READ_FRAMEBUFFER,Tt))}if(t.bindFramebuffer(n.READ_FRAMEBUFFER,null),t.bindFramebuffer(n.DRAW_FRAMEBUFFER,null),J)for(let ce=0;ce<x.length;ce++){t.bindFramebuffer(n.FRAMEBUFFER,le.__webglMultisampledFramebuffer),n.framebufferRenderbuffer(n.FRAMEBUFFER,n.COLOR_ATTACHMENT0+ce,n.RENDERBUFFER,le.__webglColorRenderbuffer[ce]);let Ie=i.get(x[ce]).__webglTexture;t.bindFramebuffer(n.FRAMEBUFFER,le.__webglFramebuffer),n.framebufferTexture2D(n.DRAW_FRAMEBUFFER,n.COLOR_ATTACHMENT0+ce,n.TEXTURE_2D,Ie,0)}t.bindFramebuffer(n.DRAW_FRAMEBUFFER,le.__webglMultisampledFramebuffer)}else if(T.depthBuffer&&T.storeMultisampledDepthBuffer===!1&&l){let x=T.stencilBuffer?n.DEPTH_STENCIL_ATTACHMENT:n.DEPTH_ATTACHMENT;n.invalidateFramebuffer(n.DRAW_FRAMEBUFFER,[x])}}}function Mt(T){return Math.min(s.maxSamples,T.samples)}function Ft(T){let x=i.get(T);return T.samples>0&&e.has("WEBGL_multisampled_render_to_texture")===!0&&x.__useRenderToTexture!==!1}function I(T){let x=o.render.frame;u.get(T)!==x&&(u.set(T,x),T.update())}function Qt(T,x){let U=T.colorSpace,z=T.format,Z=T.type;return T.isCompressedTexture===!0||T.isVideoTexture===!0||U!==Wi&&U!==Pn&&(Ve.getTransfer(U)===nt?(z!==gn||Z!==mn)&&pe("WebGLTextures: sRGB encoded textures have to use RGBAFormat and UnsignedByteType."):_e("WebGLTextures: Unsupported texture color space:",U)),x}function ut(T){return typeof HTMLImageElement<"u"&&T instanceof HTMLImageElement?(c.width=T.naturalWidth||T.width,c.height=T.naturalHeight||T.height):typeof VideoFrame<"u"&&T instanceof VideoFrame?(c.width=T.displayWidth,c.height=T.displayHeight):(c.width=T.width,c.height=T.height),c}this.allocateTextureUnit=q,this.resetTextureUnits=H,this.getTextureUnits=F,this.setTextureUnits=O,this.setTexture2D=re,this.setTexture2DArray=K,this.setTexture3D=ne,this.setTextureCube=se,this.rebindTextures=st,this.setupRenderTarget=_t,this.updateRenderTargetMipmap=Qe,this.updateMultisampleRenderTarget=xn,this.setupDepthRenderbuffer=$e,this.setupFrameBufferTexture=ve,this.useMultisampledRTT=Ft,this.isReversedDepthBuffer=function(){return t.buffers.depth.getReversed()}}function qA(n,e){function t(i,s=Pn){let r,o=Ve.getTransfer(s);if(i===mn)return n.UNSIGNED_BYTE;if(i===Bo)return n.UNSIGNED_SHORT_4_4_4_4;if(i===Fo)return n.UNSIGNED_SHORT_5_5_5_1;if(i===sc)return n.UNSIGNED_INT_5_9_9_9_REV;if(i===rc)return n.UNSIGNED_INT_10F_11F_11F_REV;if(i===nc)return n.BYTE;if(i===ic)return n.SHORT;if(i===As)return n.UNSIGNED_SHORT;if(i===Ts)return n.INT;if(i===bn)return n.UNSIGNED_INT;if(i===Dn)return n.FLOAT;if(i===Sn)return n.HALF_FLOAT;if(i===Lo)return n.ALPHA;if(i===Uo)return n.RGB;if(i===gn)return n.RGBA;if(i===Un)return n.DEPTH_COMPONENT;if(i===fi)return n.DEPTH_STENCIL;if(i===Do)return n.RED;if(i===Rr)return n.RED_INTEGER;if(i===Qn)return n.RG;if(i===wr)return n.RG_INTEGER;if(i===Po)return n.RGBA_INTEGER;if(i===Cr||i===Nr||i===Ir||i===Br)if(o===nt)if(r=e.get("WEBGL_compressed_texture_s3tc_srgb"),r!==null){if(i===Cr)return r.COMPRESSED_SRGB_S3TC_DXT1_EXT;if(i===Nr)return r.COMPRESSED_SRGB_ALPHA_S3TC_DXT1_EXT;if(i===Ir)return r.COMPRESSED_SRGB_ALPHA_S3TC_DXT3_EXT;if(i===Br)return r.COMPRESSED_SRGB_ALPHA_S3TC_DXT5_EXT}else return null;else if(r=e.get("WEBGL_compressed_texture_s3tc"),r!==null){if(i===Cr)return r.COMPRESSED_RGB_S3TC_DXT1_EXT;if(i===Nr)return r.COMPRESSED_RGBA_S3TC_DXT1_EXT;if(i===Ir)return r.COMPRESSED_RGBA_S3TC_DXT3_EXT;if(i===Br)return r.COMPRESSED_RGBA_S3TC_DXT5_EXT}else return null;if(i===Go||i===Vo||i===Oo||i===ko)if(r=e.get("WEBGL_compressed_texture_pvrtc"),r!==null){if(i===Go)return r.COMPRESSED_RGB_PVRTC_4BPPV1_IMG;if(i===Vo)return r.COMPRESSED_RGB_PVRTC_2BPPV1_IMG;if(i===Oo)return r.COMPRESSED_RGBA_PVRTC_4BPPV1_IMG;if(i===ko)return r.COMPRESSED_RGBA_PVRTC_2BPPV1_IMG}else return null;if(i===zo||i===Wo||i===Ho||i===Yo||i===Zo||i===Ms||i===Xo)if(r=e.get("WEBGL_compressed_texture_etc"),r!==null){if(i===zo||i===Wo)return o===nt?r.COMPRESSED_SRGB8_ETC2:r.COMPRESSED_RGB8_ETC2;if(i===Ho)return o===nt?r.COMPRESSED_SRGB8_ALPHA8_ETC2_EAC:r.COMPRESSED_RGBA8_ETC2_EAC;if(i===Yo)return r.COMPRESSED_R11_EAC;if(i===Zo)return r.COMPRESSED_SIGNED_R11_EAC;if(i===Ms)return r.COMPRESSED_RG11_EAC;if(i===Xo)return r.COMPRESSED_SIGNED_RG11_EAC}else return null;if(i===Ko||i===$o||i===Jo||i===qo||i===Qo||i===jo||i===ea||i===ta||i===na||i===ia||i===sa||i===ra||i===oa||i===aa)if(r=e.get("WEBGL_compressed_texture_astc"),r!==null){if(i===Ko)return o===nt?r.COMPRESSED_SRGB8_ALPHA8_ASTC_4x4_KHR:r.COMPRESSED_RGBA_ASTC_4x4_KHR;if(i===$o)return o===nt?r.COMPRESSED_SRGB8_ALPHA8_ASTC_5x4_KHR:r.COMPRESSED_RGBA_ASTC_5x4_KHR;if(i===Jo)return o===nt?r.COMPRESSED_SRGB8_ALPHA8_ASTC_5x5_KHR:r.COMPRESSED_RGBA_ASTC_5x5_KHR;if(i===qo)return o===nt?r.COMPRESSED_SRGB8_ALPHA8_ASTC_6x5_KHR:r.COMPRESSED_RGBA_ASTC_6x5_KHR;if(i===Qo)return o===nt?r.COMPRESSED_SRGB8_ALPHA8_ASTC_6x6_KHR:r.COMPRESSED_RGBA_ASTC_6x6_KHR;if(i===jo)return o===nt?r.COMPRESSED_SRGB8_ALPHA8_ASTC_8x5_KHR:r.COMPRESSED_RGBA_ASTC_8x5_KHR;if(i===ea)return o===nt?r.COMPRESSED_SRGB8_ALPHA8_ASTC_8x6_KHR:r.COMPRESSED_RGBA_ASTC_8x6_KHR;if(i===ta)return o===nt?r.COMPRESSED_SRGB8_ALPHA8_ASTC_8x8_KHR:r.COMPRESSED_RGBA_ASTC_8x8_KHR;if(i===na)return o===nt?r.COMPRESSED_SRGB8_ALPHA8_ASTC_10x5_KHR:r.COMPRESSED_RGBA_ASTC_10x5_KHR;if(i===ia)return o===nt?r.COMPRESSED_SRGB8_ALPHA8_ASTC_10x6_KHR:r.COMPRESSED_RGBA_ASTC_10x6_KHR;if(i===sa)return o===nt?r.COMPRESSED_SRGB8_ALPHA8_ASTC_10x8_KHR:r.COMPRESSED_RGBA_ASTC_10x8_KHR;if(i===ra)return o===nt?r.COMPRESSED_SRGB8_ALPHA8_ASTC_10x10_KHR:r.COMPRESSED_RGBA_ASTC_10x10_KHR;if(i===oa)return o===nt?r.COMPRESSED_SRGB8_ALPHA8_ASTC_12x10_KHR:r.COMPRESSED_RGBA_ASTC_12x10_KHR;if(i===aa)return o===nt?r.COMPRESSED_SRGB8_ALPHA8_ASTC_12x12_KHR:r.COMPRESSED_RGBA_ASTC_12x12_KHR}else return null;if(i===la||i===ca||i===ua)if(r=e.get("EXT_texture_compression_bptc"),r!==null){if(i===la)return o===nt?r.COMPRESSED_SRGB_ALPHA_BPTC_UNORM_EXT:r.COMPRESSED_RGBA_BPTC_UNORM_EXT;if(i===ca)return r.COMPRESSED_RGB_BPTC_SIGNED_FLOAT_EXT;if(i===ua)return r.COMPRESSED_RGB_BPTC_UNSIGNED_FLOAT_EXT}else return null;if(i===ha||i===da||i===Rs||i===fa)if(r=e.get("EXT_texture_compression_rgtc"),r!==null){if(i===ha)return r.COMPRESSED_RED_RGTC1_EXT;if(i===da)return r.COMPRESSED_SIGNED_RED_RGTC1_EXT;if(i===Rs)return r.COMPRESSED_RED_GREEN_RGTC2_EXT;if(i===fa)return r.COMPRESSED_SIGNED_RED_GREEN_RGTC2_EXT}else return null;return i===Es?n.UNSIGNED_INT_24_8:n[i]!==void 0?n[i]:null}return{convert:t}}var QA=`
void main() {

	gl_Position = vec4( position, 1.0 );

}`,jA=`
uniform sampler2DArray depthColor;
uniform float depthWidth;
uniform float depthHeight;

void main() {

	vec2 coord = vec2( gl_FragCoord.x / depthWidth, gl_FragCoord.y / depthHeight );

	if ( coord.x >= 1.0 ) {

		gl_FragDepth = texture( depthColor, vec3( coord.x - 1.0, coord.y, 1 ) ).r;

	} else {

		gl_FragDepth = texture( depthColor, vec3( coord.x, coord.y, 0 ) ).r;

	}

}`,qh=class{constructor(){this.texture=null,this.mesh=null,this.depthNear=0,this.depthFar=0}init(e,t){if(this.texture===null){let i=new So(e.texture);(e.depthNear!==t.depthNear||e.depthFar!==t.depthFar)&&(this.depthNear=e.depthNear,this.depthFar=e.depthFar),this.texture=i}}getMesh(e){if(this.texture!==null&&this.mesh===null){let t=e.cameras[0].viewport,i=new Cn({vertexShader:QA,fragmentShader:jA,uniforms:{depthColor:{value:this.texture},depthWidth:{value:t.z},depthHeight:{value:t.w}}});this.mesh=new fn(new br(20,20),i)}return this.mesh}reset(){this.texture=null,this.mesh=null}getDepthTexture(){return this.texture}},Qh=class extends wn{constructor(e,t){super();let i=this,s=null,r=1,o=null,a="local-floor",l=1,c=null,u=null,d=null,h=null,f=null,g=null,y=typeof XRWebGLBinding<"u",m=new qh,p={},E=t.getContextAttributes(),w=null,S=null,v=[],A=[],R=new ze,b=null,M=null,B=new Kt;B.viewport=new dt;let D=new Kt;D.viewport=new dt;let k=[B,D],H=new Ro,F=null,O=null;this.cameraAutoUpdate=!0,this.enabled=!1,this.isPresenting=!1,this.getController=function($){let te=v[$];return te===void 0&&(te=new fs,v[$]=te),te.getTargetRaySpace()},this.getControllerGrip=function($){let te=v[$];return te===void 0&&(te=new fs,v[$]=te),te.getGripSpace()},this.getHand=function($){let te=v[$];return te===void 0&&(te=new fs,v[$]=te),te.getHandSpace()};function q($){let te=A.indexOf($.inputSource);if(te===-1)return;let Te=v[te];Te!==void 0&&(Te.update($.inputSource,$.frame,c||o),Te.dispatchEvent({type:$.type,data:$.inputSource}))}function X(){s.removeEventListener("select",q),s.removeEventListener("selectstart",q),s.removeEventListener("selectend",q),s.removeEventListener("squeeze",q),s.removeEventListener("squeezestart",q),s.removeEventListener("squeezeend",q),s.removeEventListener("end",X),s.removeEventListener("inputsourceschange",re);for(let $=0;$<v.length;$++){let te=A[$];te!==null&&(A[$]=null,v[$].disconnect(te))}F=null,O=null,m.reset();for(let $ in p)delete p[$];if(e.setRenderTarget(w),f=null,h=null,d=null,s=null,S=null,rt.stop(),i.isPresenting=!1,e.setPixelRatio(b),e.setSize(R.width,R.height,!1),M!==null){let $=M.camera;$.fov=M.fov,$.zoom=M.zoom,$.updateProjectionMatrix(),M=null}i.dispatchEvent({type:"sessionend"})}this.setFramebufferScaleFactor=function($){r=$,i.isPresenting===!0&&pe("WebXRManager: Cannot change framebuffer scale while presenting.")},this.setReferenceSpaceType=function($){a=$,i.isPresenting===!0&&pe("WebXRManager: Cannot change reference space type while presenting.")},this.getReferenceSpace=function(){return c||o},this.setReferenceSpace=function($){c=$},this.getBaseLayer=function(){return h!==null?h:f},this.getBinding=function(){return d===null&&y&&(d=new XRWebGLBinding(s,t)),d},this.getFrame=function(){return g},this.getSession=function(){return s},this.setSession=async function($){if(s=$,s!==null){if(w=e.getRenderTarget(),s.addEventListener("select",q),s.addEventListener("selectstart",q),s.addEventListener("selectend",q),s.addEventListener("squeeze",q),s.addEventListener("squeezestart",q),s.addEventListener("squeezeend",q),s.addEventListener("end",X),s.addEventListener("inputsourceschange",re),E.xrCompatible!==!0&&await t.makeXRCompatible(),b=e.getPixelRatio(),e.getSize(R),y&&"createProjectionLayer"in XRWebGLBinding.prototype){let Te=null,Oe=null,ve=null;E.depth&&(ve=E.stencil?t.DEPTH24_STENCIL8:t.DEPTH_COMPONENT24,Te=E.stencil?fi:Un,Oe=E.stencil?Es:bn);let Ke={colorFormat:t.RGBA8,depthFormat:ve,scaleFactor:r};d=this.getBinding(),h=d.createProjectionLayer(Ke),s.updateRenderState({layers:[h]}),e.setPixelRatio(1),e.setSize(h.textureWidth,h.textureHeight,!1),S=new _n(h.textureWidth,h.textureHeight,{format:gn,type:mn,depthTexture:new $n(h.textureWidth,h.textureHeight,Oe,void 0,void 0,void 0,void 0,void 0,void 0,Te),stencilBuffer:E.stencil,colorSpace:e.outputColorSpace,samples:E.antialias?4:0,resolveDepthBuffer:h.ignoreDepthValues===!1,resolveStencilBuffer:h.ignoreDepthValues===!1,storeMultisampledDepthBuffer:h.ignoreDepthValues===!1,storeMultisampledStencilBuffer:h.ignoreDepthValues===!1})}else{let Te={antialias:E.antialias,alpha:!0,depth:E.depth,stencil:E.stencil,framebufferScaleFactor:r};f=new XRWebGLLayer(s,t,Te),s.updateRenderState({baseLayer:f}),e.setPixelRatio(1),e.setSize(f.framebufferWidth,f.framebufferHeight,!1),S=new _n(f.framebufferWidth,f.framebufferHeight,{format:gn,type:mn,colorSpace:e.outputColorSpace,stencilBuffer:E.stencil,resolveDepthBuffer:f.ignoreDepthValues===!1,resolveStencilBuffer:f.ignoreDepthValues===!1,storeMultisampledDepthBuffer:f.ignoreDepthValues===!1,storeMultisampledStencilBuffer:f.ignoreDepthValues===!1})}S.isXRRenderTarget=!0,this.setFoveation(l),c=null,o=await s.requestReferenceSpace(a),rt.setContext(s),rt.start(),i.isPresenting=!0,i.dispatchEvent({type:"sessionstart"})}},this.getEnvironmentBlendMode=function(){if(s!==null)return s.environmentBlendMode},this.getDepthTexture=function(){return m.getDepthTexture()};function re($){for(let te=0;te<$.removed.length;te++){let Te=$.removed[te],Oe=A.indexOf(Te);Oe>=0&&(A[Oe]=null,v[Oe].disconnect(Te))}for(let te=0;te<$.added.length;te++){let Te=$.added[te],Oe=A.indexOf(Te);if(Oe===-1){for(let Ke=0;Ke<v.length;Ke++)if(Ke>=A.length){A.push(Te),Oe=Ke;break}else if(A[Ke]===null){A[Ke]=Te,Oe=Ke;break}if(Oe===-1)break}let ve=v[Oe];ve&&ve.connect(Te)}}let K=new L,ne=new L;function se($,te,Te){K.setFromMatrixPosition(te.matrixWorld),ne.setFromMatrixPosition(Te.matrixWorld);let Oe=K.distanceTo(ne),ve=te.projectionMatrix.elements,Ke=Te.projectionMatrix.elements,Ot=ve[14]/(ve[10]-1),$e=ve[14]/(ve[10]+1),st=(ve[9]+1)/ve[5],_t=(ve[9]-1)/ve[5],Qe=(ve[8]-1)/ve[0],Tt=(Ke[8]+1)/Ke[0],Yt=Ot*Qe,xn=Ot*Tt,Mt=Oe/(-Qe+Tt),Ft=Mt*-Qe;if(te.matrixWorld.decompose($.position,$.quaternion,$.scale),$.translateX(Ft),$.translateZ(Mt),$.matrixWorld.compose($.position,$.quaternion,$.scale),$.matrixWorldInverse.copy($.matrixWorld).invert(),ve[10]===-1)$.projectionMatrix.copy(te.projectionMatrix),$.projectionMatrixInverse.copy(te.projectionMatrixInverse);else{let I=Ot+Mt,Qt=$e+Mt,ut=Yt-Ft,T=xn+(Oe-Ft),x=st*$e/Qt*I,U=_t*$e/Qt*I;$.projectionMatrix.makePerspective(ut,T,x,U,I,Qt),$.projectionMatrixInverse.copy($.projectionMatrix).invert()}}function Ue($,te){te===null?$.matrixWorld.copy($.matrix):$.matrixWorld.multiplyMatrices(te.matrixWorld,$.matrix),$.matrixWorldInverse.copy($.matrixWorld).invert()}this.updateCamera=function($){if(s===null)return;let te=$.near,Te=$.far;m.texture!==null&&(m.depthNear>0&&(te=m.depthNear),m.depthFar>0&&(Te=m.depthFar)),H.near=D.near=B.near=te,H.far=D.far=B.far=Te,(F!==H.near||O!==H.far)&&(s.updateRenderState({depthNear:H.near,depthFar:H.far}),F=H.near,O=H.far),H.layers.mask=$.layers.mask|6,B.layers.mask=H.layers.mask&-5,D.layers.mask=H.layers.mask&-3;let Oe=$.parent,ve=H.cameras;Ue(H,Oe);for(let Ke=0;Ke<ve.length;Ke++)Ue(ve[Ke],Oe);ve.length===2?se(H,B,D):H.projectionMatrix.copy(B.projectionMatrix),M===null&&$.isPerspectiveCamera&&(M={camera:$,fov:$.fov,zoom:$.zoom}),Fe($,H,Oe)};function Fe($,te,Te){Te===null?$.matrix.copy(te.matrixWorld):($.matrix.copy(Te.matrixWorld),$.matrix.invert(),$.matrix.multiply(te.matrixWorld)),$.matrix.decompose($.position,$.quaternion,$.scale),$.updateMatrixWorld(!0),$.projectionMatrix.copy(te.projectionMatrix),$.projectionMatrixInverse.copy(te.projectionMatrixInverse),$.isPerspectiveCamera&&($.fov=ds*2*Math.atan(1/$.projectionMatrix.elements[5]),$.zoom=1)}this.getCamera=function(){return H},this.getFoveation=function(){if(!(h===null&&f===null))return l},this.setFoveation=function($){l=$,h!==null&&(h.fixedFoveation=$),f!==null&&f.fixedFoveation!==void 0&&(f.fixedFoveation=$)},this.hasDepthSensing=function(){return m.texture!==null},this.getDepthSensingMesh=function(){return m.getMesh(H)},this.getCameraTexture=function($){return p[$]};let yt=null;function je($,te){if(u=te.getViewerPose(c||o),g=te,u!==null){let Te=u.views;f!==null&&(e.setRenderTargetFramebuffer(S,f.framebuffer),e.setRenderTarget(S));let Oe=!1;Te.length!==H.cameras.length&&(H.cameras.length=0,Oe=!0);for(let $e=0;$e<Te.length;$e++){let st=Te[$e],_t=null;if(f!==null)_t=f.getViewport(st);else{let Tt=d.getViewSubImage(h,st);_t=Tt.viewport,$e===0&&(e.setRenderTargetTextures(S,Tt.colorTexture,Tt.depthStencilTexture),e.setRenderTarget(S))}let Qe=k[$e];Qe===void 0&&(Qe=new Kt,Qe.layers.enable($e),Qe.viewport=new dt,k[$e]=Qe),Qe.matrix.fromArray(st.transform.matrix),Qe.matrix.decompose(Qe.position,Qe.quaternion,Qe.scale),Qe.projectionMatrix.fromArray(st.projectionMatrix),Qe.projectionMatrixInverse.copy(Qe.projectionMatrix).invert(),Qe.viewport.set(_t.x,_t.y,_t.width,_t.height),$e===0&&(H.matrix.copy(Qe.matrix),H.matrix.decompose(H.position,H.quaternion,H.scale)),Oe===!0&&H.cameras.push(Qe)}let ve=s.enabledFeatures;if(ve&&ve.includes("depth-sensing")&&s.depthUsage=="gpu-optimized"&&y){d=i.getBinding();let $e=d.getDepthInformation(Te[0]);$e&&$e.isValid&&$e.texture&&m.init($e,s.renderState)}if(ve&&ve.includes("camera-access")&&y){e.state.unbindTexture(),d=i.getBinding();for(let $e=0;$e<Te.length;$e++){let st=Te[$e].camera;if(st){let _t=p[st];_t||(_t=new So,p[st]=_t);let Qe=d.getCameraImage(st);_t.sourceTexture=Qe}}}}for(let Te=0;Te<v.length;Te++){let Oe=A[Te],ve=v[Te];Oe!==null&&ve!==void 0&&ve.update(Oe,te,c||o)}yt&&yt($,te),te.detectedPlanes&&i.dispatchEvent({type:"planesdetected",data:te}),g=null}let rt=new Rm;rt.setAnimationLoop(je),this.setAnimationLoop=function($){yt=$},this.dispose=function(){}}},eT=new ct,Fm=new Ge;Fm.set(-1,0,0,0,1,0,0,0,1);function tT(n,e){function t(m,p){m.matrixAutoUpdate===!0&&m.updateMatrix(),p.value.copy(m.matrix)}function i(m,p){p.color.getRGB(m.fogColor.value,Uh(n)),p.isFog?(m.fogNear.value=p.near,m.fogFar.value=p.far):p.isFogExp2&&(m.fogDensity.value=p.density)}function s(m,p,E,w,S){p.isNodeMaterial?p.uniformsNeedUpdate=!1:p.isMeshBasicMaterial?r(m,p):p.isMeshLambertMaterial?(r(m,p),p.envMap&&(m.envMapIntensity.value=p.envMapIntensity)):p.isMeshToonMaterial?(r(m,p),d(m,p)):p.isMeshPhongMaterial?(r(m,p),u(m,p),p.envMap&&(m.envMapIntensity.value=p.envMapIntensity)):p.isMeshStandardMaterial?(r(m,p),h(m,p),p.isMeshPhysicalMaterial&&f(m,p,S)):p.isMeshMatcapMaterial?(r(m,p),g(m,p)):p.isMeshDepthMaterial?r(m,p):p.isMeshDistanceMaterial?(r(m,p),y(m,p)):p.isMeshNormalMaterial?r(m,p):p.isLineBasicMaterial?(o(m,p),p.isLineDashedMaterial&&a(m,p)):p.isPointsMaterial?l(m,p,E,w):p.isSpriteMaterial?c(m,p):p.isShadowMaterial?(m.color.value.copy(p.color),m.opacity.value=p.opacity):p.isShaderMaterial&&(p.uniformsNeedUpdate=!1)}function r(m,p){m.opacity.value=p.opacity,p.color&&m.diffuse.value.copy(p.color),p.emissive&&m.emissive.value.copy(p.emissive).multiplyScalar(p.emissiveIntensity),p.map&&(m.map.value=p.map,t(p.map,m.mapTransform)),p.alphaMap&&(m.alphaMap.value=p.alphaMap,t(p.alphaMap,m.alphaMapTransform)),p.bumpMap&&(m.bumpMap.value=p.bumpMap,t(p.bumpMap,m.bumpMapTransform),m.bumpScale.value=p.bumpScale,p.side===Dt&&(m.bumpScale.value*=-1)),p.normalMap&&(m.normalMap.value=p.normalMap,t(p.normalMap,m.normalMapTransform),m.normalScale.value.copy(p.normalScale),p.side===Dt&&m.normalScale.value.negate()),p.displacementMap&&(m.displacementMap.value=p.displacementMap,t(p.displacementMap,m.displacementMapTransform),m.displacementScale.value=p.displacementScale,m.displacementBias.value=p.displacementBias),p.emissiveMap&&(m.emissiveMap.value=p.emissiveMap,t(p.emissiveMap,m.emissiveMapTransform)),p.specularMap&&(m.specularMap.value=p.specularMap,t(p.specularMap,m.specularMapTransform)),p.alphaTest>0&&(m.alphaTest.value=p.alphaTest);let E=e.get(p),w=E.envMap,S=E.envMapRotation;w&&(m.envMap.value=w,m.envMapRotation.value.setFromMatrix4(eT.makeRotationFromEuler(S)).transpose(),w.isCubeTexture&&w.isRenderTargetTexture===!1&&m.envMapRotation.value.premultiply(Fm),m.reflectivity.value=p.reflectivity,m.ior.value=p.ior,m.refractionRatio.value=p.refractionRatio),p.lightMap&&(m.lightMap.value=p.lightMap,m.lightMapIntensity.value=p.lightMapIntensity,t(p.lightMap,m.lightMapTransform)),p.aoMap&&(m.aoMap.value=p.aoMap,m.aoMapIntensity.value=p.aoMapIntensity,t(p.aoMap,m.aoMapTransform))}function o(m,p){m.diffuse.value.copy(p.color),m.opacity.value=p.opacity,p.map&&(m.map.value=p.map,t(p.map,m.mapTransform))}function a(m,p){m.dashSize.value=p.dashSize,m.totalSize.value=p.dashSize+p.gapSize,m.scale.value=p.scale}function l(m,p,E,w){m.diffuse.value.copy(p.color),m.opacity.value=p.opacity,m.size.value=p.size*E,m.scale.value=w*.5,p.map&&(m.map.value=p.map,t(p.map,m.uvTransform)),p.alphaMap&&(m.alphaMap.value=p.alphaMap,t(p.alphaMap,m.alphaMapTransform)),p.alphaTest>0&&(m.alphaTest.value=p.alphaTest)}function c(m,p){m.diffuse.value.copy(p.color),m.opacity.value=p.opacity,m.rotation.value=p.rotation,p.map&&(m.map.value=p.map,t(p.map,m.mapTransform)),p.alphaMap&&(m.alphaMap.value=p.alphaMap,t(p.alphaMap,m.alphaMapTransform)),p.alphaTest>0&&(m.alphaTest.value=p.alphaTest)}function u(m,p){m.specular.value.copy(p.specular),m.shininess.value=Math.max(p.shininess,1e-4)}function d(m,p){p.gradientMap&&(m.gradientMap.value=p.gradientMap)}function h(m,p){m.metalness.value=p.metalness,p.metalnessMap&&(m.metalnessMap.value=p.metalnessMap,t(p.metalnessMap,m.metalnessMapTransform)),m.roughness.value=p.roughness,p.roughnessMap&&(m.roughnessMap.value=p.roughnessMap,t(p.roughnessMap,m.roughnessMapTransform)),p.envMap&&(m.envMapIntensity.value=p.envMapIntensity)}function f(m,p,E){m.ior.value=p.ior,p.sheen>0&&(m.sheenColor.value.copy(p.sheenColor).multiplyScalar(p.sheen),m.sheenRoughness.value=p.sheenRoughness,p.sheenColorMap&&(m.sheenColorMap.value=p.sheenColorMap,t(p.sheenColorMap,m.sheenColorMapTransform)),p.sheenRoughnessMap&&(m.sheenRoughnessMap.value=p.sheenRoughnessMap,t(p.sheenRoughnessMap,m.sheenRoughnessMapTransform))),p.clearcoat>0&&(m.clearcoat.value=p.clearcoat,m.clearcoatRoughness.value=p.clearcoatRoughness,p.clearcoatMap&&(m.clearcoatMap.value=p.clearcoatMap,t(p.clearcoatMap,m.clearcoatMapTransform)),p.clearcoatRoughnessMap&&(m.clearcoatRoughnessMap.value=p.clearcoatRoughnessMap,t(p.clearcoatRoughnessMap,m.clearcoatRoughnessMapTransform)),p.clearcoatNormalMap&&(m.clearcoatNormalMap.value=p.clearcoatNormalMap,t(p.clearcoatNormalMap,m.clearcoatNormalMapTransform),m.clearcoatNormalScale.value.copy(p.clearcoatNormalScale),p.side===Dt&&m.clearcoatNormalScale.value.negate())),p.dispersion>0&&(m.dispersion.value=p.dispersion),p.retroreflectivity>0&&(m.retroreflectivity.value=p.retroreflectivity),p.iridescence>0&&(m.iridescence.value=p.iridescence,m.iridescenceIOR.value=p.iridescenceIOR,m.iridescenceThicknessMinimum.value=p.iridescenceThicknessRange[0],m.iridescenceThicknessMaximum.value=p.iridescenceThicknessRange[1],p.iridescenceMap&&(m.iridescenceMap.value=p.iridescenceMap,t(p.iridescenceMap,m.iridescenceMapTransform)),p.iridescenceThicknessMap&&(m.iridescenceThicknessMap.value=p.iridescenceThicknessMap,t(p.iridescenceThicknessMap,m.iridescenceThicknessMapTransform))),p.transmission>0&&(m.transmission.value=p.transmission,m.transmissionSamplerMap.value=E.texture,m.transmissionSamplerSize.value.set(E.width,E.height),p.transmissionMap&&(m.transmissionMap.value=p.transmissionMap,t(p.transmissionMap,m.transmissionMapTransform)),m.thickness.value=p.thickness,p.thicknessMap&&(m.thicknessMap.value=p.thicknessMap,t(p.thicknessMap,m.thicknessMapTransform)),m.attenuationDistance.value=p.attenuationDistance,m.attenuationColor.value.copy(p.attenuationColor)),p.anisotropy>0&&(m.anisotropyVector.value.set(p.anisotropy*Math.cos(p.anisotropyRotation),p.anisotropy*Math.sin(p.anisotropyRotation)),p.anisotropyMap&&(m.anisotropyMap.value=p.anisotropyMap,t(p.anisotropyMap,m.anisotropyMapTransform))),m.specularIntensity.value=p.specularIntensity,m.specularColor.value.copy(p.specularColor),p.specularColorMap&&(m.specularColorMap.value=p.specularColorMap,t(p.specularColorMap,m.specularColorMapTransform)),p.specularIntensityMap&&(m.specularIntensityMap.value=p.specularIntensityMap,t(p.specularIntensityMap,m.specularIntensityMapTransform))}function g(m,p){p.matcap&&(m.matcap.value=p.matcap)}function y(m,p){let E=e.get(p).light;m.referencePosition.value.setFromMatrixPosition(E.matrixWorld),m.nearDistance.value=E.shadow.camera.near,m.farDistance.value=E.shadow.camera.far}return{refreshFogUniforms:i,refreshMaterialUniforms:s}}function nT(n,e,t,i){let s={},r={},o=[],a=n.getParameter(n.MAX_UNIFORM_BUFFER_BINDINGS);function l(S,v){let A=v.program;i.uniformBlockBinding(S,A)}function c(S,v){let A=s[S.id];A===void 0&&(m(S),A=u(S),s[S.id]=A,S.addEventListener("dispose",E));let R=v.program;i.updateUBOMapping(S,R);let b=e.render.frame;r[S.id]!==b&&(h(S),r[S.id]=b)}function u(S){let v=d();S.__bindingPointIndex=v;let A=n.createBuffer(),R=S.__size,b=S.usage;return n.bindBuffer(n.UNIFORM_BUFFER,A),n.bufferData(n.UNIFORM_BUFFER,R,b),n.bindBuffer(n.UNIFORM_BUFFER,null),n.bindBufferBase(n.UNIFORM_BUFFER,v,A),A}function d(){for(let S=0;S<a;S++)if(o.indexOf(S)===-1)return o.push(S),S;return _e("WebGLRenderer: Maximum number of simultaneously usable uniforms groups reached."),0}function h(S){let v=s[S.id],A=S.uniforms,R=S.__cache;n.bindBuffer(n.UNIFORM_BUFFER,v);for(let b=0,M=A.length;b<M;b++){let B=A[b];if(Array.isArray(B))for(let D=0,k=B.length;D<k;D++)f(B[D],b,D,R);else f(B,b,0,R)}n.bindBuffer(n.UNIFORM_BUFFER,null)}function f(S,v,A,R){if(y(S,v,A,R)===!0){let b=S.__offset,M=S.value;if(Array.isArray(M)){let B=0;for(let D=0;D<M.length;D++){let k=M[D],H=p(k);g(k,S.__data,B),typeof k!="number"&&typeof k!="boolean"&&!k.isMatrix3&&!ArrayBuffer.isView(k)&&(B+=H.storage/Float32Array.BYTES_PER_ELEMENT)}}else g(M,S.__data,0);n.bufferSubData(n.UNIFORM_BUFFER,b,S.__data)}}function g(S,v,A){typeof S=="number"||typeof S=="boolean"?v[0]=S:S.isMatrix3?(v[0]=S.elements[0],v[1]=S.elements[1],v[2]=S.elements[2],v[3]=0,v[4]=S.elements[3],v[5]=S.elements[4],v[6]=S.elements[5],v[7]=0,v[8]=S.elements[6],v[9]=S.elements[7],v[10]=S.elements[8],v[11]=0):ArrayBuffer.isView(S)?v.set(new S.constructor(S.buffer,S.byteOffset,v.length)):S.toArray(v,A)}function y(S,v,A,R){let b=S.value,M=v+"_"+A;if(R[M]===void 0)return typeof b=="number"||typeof b=="boolean"?R[M]=b:ArrayBuffer.isView(b)?R[M]=b.slice():R[M]=b.clone(),!0;{let B=R[M];if(typeof b=="number"||typeof b=="boolean"){if(B!==b)return R[M]=b,!0}else{if(ArrayBuffer.isView(b))return!0;if(B.equals(b)===!1)return B.copy(b),!0}}return!1}function m(S){let v=S.uniforms,A=0,R=16;for(let M=0,B=v.length;M<B;M++){let D=Array.isArray(v[M])?v[M]:[v[M]];for(let k=0,H=D.length;k<H;k++){let F=D[k],O=Array.isArray(F.value)?F.value:[F.value];for(let q=0,X=O.length;q<X;q++){let re=O[q],K=p(re),ne=A%R,se=ne%K.boundary,Ue=ne+se;A+=se,Ue!==0&&R-Ue<K.storage&&(A+=R-Ue),F.__data=new Float32Array(K.storage/Float32Array.BYTES_PER_ELEMENT),F.__offset=A,A+=K.storage}}}let b=A%R;return b>0&&(A+=R-b),S.__size=A,S.__cache={},this}function p(S){let v={boundary:0,storage:0};return typeof S=="number"||typeof S=="boolean"?(v.boundary=4,v.storage=4):S.isVector2?(v.boundary=8,v.storage=8):S.isVector3||S.isColor?(v.boundary=16,v.storage=12):S.isVector4?(v.boundary=16,v.storage=16):S.isMatrix3?(v.boundary=48,v.storage=48):S.isMatrix4?(v.boundary=64,v.storage=64):S.isTexture?pe("WebGLRenderer: Texture samplers can not be part of an uniforms group."):ArrayBuffer.isView(S)?(v.boundary=16,v.storage=S.byteLength):pe("WebGLRenderer: Unsupported uniform value type.",S),v}function E(S){let v=S.target;v.removeEventListener("dispose",E);let A=o.indexOf(v.__bindingPointIndex);o.splice(A,1),n.deleteBuffer(s[v.id]),delete s[v.id],delete r[v.id]}function w(){for(let S in s)n.deleteBuffer(s[S]);o=[],s={},r={}}return{bind:l,update:c,dispose:w}}var iT=new Uint16Array([12469,15057,12620,14925,13266,14620,13807,14376,14323,13990,14545,13625,14713,13328,14840,12882,14931,12528,14996,12233,15039,11829,15066,11525,15080,11295,15085,10976,15082,10705,15073,10495,13880,14564,13898,14542,13977,14430,14158,14124,14393,13732,14556,13410,14702,12996,14814,12596,14891,12291,14937,11834,14957,11489,14958,11194,14943,10803,14921,10506,14893,10278,14858,9960,14484,14039,14487,14025,14499,13941,14524,13740,14574,13468,14654,13106,14743,12678,14818,12344,14867,11893,14889,11509,14893,11180,14881,10751,14852,10428,14812,10128,14765,9754,14712,9466,14764,13480,14764,13475,14766,13440,14766,13347,14769,13070,14786,12713,14816,12387,14844,11957,14860,11549,14868,11215,14855,10751,14825,10403,14782,10044,14729,9651,14666,9352,14599,9029,14967,12835,14966,12831,14963,12804,14954,12723,14936,12564,14917,12347,14900,11958,14886,11569,14878,11247,14859,10765,14828,10401,14784,10011,14727,9600,14660,9289,14586,8893,14508,8533,15111,12234,15110,12234,15104,12216,15092,12156,15067,12010,15028,11776,14981,11500,14942,11205,14902,10752,14861,10393,14812,9991,14752,9570,14682,9252,14603,8808,14519,8445,14431,8145,15209,11449,15208,11451,15202,11451,15190,11438,15163,11384,15117,11274,15055,10979,14994,10648,14932,10343,14871,9936,14803,9532,14729,9218,14645,8742,14556,8381,14461,8020,14365,7603,15273,10603,15272,10607,15267,10619,15256,10631,15231,10614,15182,10535,15118,10389,15042,10167,14963,9787,14883,9447,14800,9115,14710,8665,14615,8318,14514,7911,14411,7507,14279,7198,15314,9675,15313,9683,15309,9712,15298,9759,15277,9797,15229,9773,15166,9668,15084,9487,14995,9274,14898,8910,14800,8539,14697,8234,14590,7790,14479,7409,14367,7067,14178,6621,15337,8619,15337,8631,15333,8677,15325,8769,15305,8871,15264,8940,15202,8909,15119,8775,15022,8565,14916,8328,14804,8009,14688,7614,14569,7287,14448,6888,14321,6483,14088,6171,15350,7402,15350,7419,15347,7480,15340,7613,15322,7804,15287,7973,15229,8057,15148,8012,15046,7846,14933,7611,14810,7357,14682,7069,14552,6656,14421,6316,14251,5948,14007,5528,15356,5942,15356,5977,15353,6119,15348,6294,15332,6551,15302,6824,15249,7044,15171,7122,15070,7050,14949,6861,14818,6611,14679,6349,14538,6067,14398,5651,14189,5311,13935,4958,15359,4123,15359,4153,15356,4296,15353,4646,15338,5160,15311,5508,15263,5829,15188,6042,15088,6094,14966,6001,14826,5796,14678,5543,14527,5287,14377,4985,14133,4586,13869,4257,15360,1563,15360,1642,15358,2076,15354,2636,15341,3350,15317,4019,15273,4429,15203,4732,15105,4911,14981,4932,14836,4818,14679,4621,14517,4386,14359,4156,14083,3795,13808,3437,15360,122,15360,137,15358,285,15355,636,15344,1274,15322,2177,15281,2765,15215,3223,15120,3451,14995,3569,14846,3567,14681,3466,14511,3305,14344,3121,14037,2800,13753,2467,15360,0,15360,1,15359,21,15355,89,15346,253,15325,479,15287,796,15225,1148,15133,1492,15008,1749,14856,1882,14685,1886,14506,1783,14324,1608,13996,1398,13702,1183]),pi=null;function sT(){return pi===null&&(pi=new ps(iT,16,16,Qn,Sn),pi.name="DFG_LUT",pi.minFilter=Bt,pi.magFilter=Bt,pi.wrapS=Ln,pi.wrapT=Ln,pi.generateMipmaps=!1,pi.needsUpdate=!0),pi}var xc=class{constructor(e={}){let{canvas:t=Ih(),context:i=null,depth:s=!0,stencil:r=!1,alpha:o=!1,antialias:a=!1,premultipliedAlpha:l=!0,preserveDrawingBuffer:c=!1,powerPreference:u="default",failIfMajorPerformanceCaveat:d=!1,reversedDepthBuffer:h=!1,outputBufferType:f=mn}=e;this.isWebGLRenderer=!0;let g;if(i!==null){if(typeof WebGLRenderingContext<"u"&&i instanceof WebGLRenderingContext)throw new Error("THREE.WebGLRenderer: WebGL 1 is not supported since r163.");g=i.getContextAttributes().alpha}else g=o;let y=f,m=new Set([Po,wr,Rr]),p=new Set([mn,bn,As,Es,Bo,Fo]),E=new Uint32Array(4),w=new Int32Array(4),S=new L,v=null,A=null,R=[],b=[],M=null;this.domElement=t,this.debug={checkShaderErrors:!0,diagnostics:{keywords:!1},onShaderError:null},this.autoClear=!0,this.autoClearColor=!0,this.autoClearDepth=!0,this.autoClearStencil=!0,this.sortObjects=!0,this.clippingPlanes=[],this.localClippingEnabled=!1,this.toneMapping=pn,this.toneMappingExposure=1,this.transmissionResolutionScale=1;let B=this,D=!1,k=null,H=null,F=null,O=null;this._outputColorSpace=yn;let q=0,X=0,re=null,K=-1,ne=null,se=new dt,Ue=new dt,Fe=null,yt=new He(0),je=0,rt=t.width,$=t.height,te=1,Te=null,Oe=null,ve=new dt(0,0,rt,$),Ke=new dt(0,0,rt,$),Ot=!1,$e=new ms,st=!1,_t=!1,Qe=new ct,Tt=new L,Yt=new dt,xn={background:null,fog:null,environment:null,overrideMaterial:null,isScene:!0},Mt=!1;function Ft(){return re===null?te:1}let I=i;function Qt(_,C){return t.getContext(_,C)}let ut,T,x,U,z,Z,ae,le,J,j,ce,Ie,fe,ue,Be,Pe,We,N,he,Q,de,be,ie;try{let _={alpha:!0,depth:s,stencil:r,antialias:a,premultipliedAlpha:l,preserveDrawingBuffer:c,powerPreference:u,failIfMajorPerformanceCaveat:d};if("setAttribute"in t&&t.setAttribute("data-engine",`three.js r${"186"}`),t.addEventListener("webglcontextlost",bt,!1),t.addEventListener("webglcontextrestored",at,!1),t.addEventListener("webglcontextcreationerror",On,!1),I===null){let C="webgl2";if(I=Qt(C,_),I===null)throw Qt(C)?new Error("THREE.WebGLRenderer: Error creating WebGL context with your selected attributes."):new Error("THREE.WebGLRenderer: Error creating WebGL context.")}Le()}catch(_){throw t.removeEventListener("webglcontextlost",bt,!1),t.removeEventListener("webglcontextrestored",at,!1),t.removeEventListener("webglcontextcreationerror",On,!1),_e("WebGLRenderer: "+_.message),_}function Le(){ut=new hv(I),ut.init(),de=new qA(I,ut),T=new tv(I,ut,e,de),x=new $A(I,ut),T.reversedDepthBuffer&&h&&x.buffers.depth.setReversed(!0),H=I.createFramebuffer(),F=I.createFramebuffer(),O=I.createFramebuffer(),U=new pv(I),z=new UA,Z=new JA(I,ut,x,z,T,de,U),ae=new uv(B),le=new g_(I),be=new jS(I,le),J=new dv(I,le,U,be),j=new gv(I,J,le,be,U),N=new mv(I,T,Z),Be=new nv(z),ce=new LA(B,ae,ut,T,be,Be),Ie=new tT(B,z),fe=new PA,ue=new WA(ut),We=new QS(B,ae,x,j,g,l),Pe=new KA(B,j,T),ie=new nT(I,U,T,x),he=new ev(I,ut,U),Q=new fv(I,ut,U),U.programs=ce.programs,B.capabilities=T,B.extensions=ut,B.properties=z,B.renderLists=fe,B.shadowMap=Pe,B.state=x,B.info=U}y!==mn&&(M=new yv(y,t.width,t.height,a,s,r));let Re=new Qh(B,I);this.xr=Re,this.getContext=function(){return I},this.getContextAttributes=function(){return I.getContextAttributes()},this.forceContextLoss=function(){let _=ut.get("WEBGL_lose_context");_&&_.loseContext()},this.forceContextRestore=function(){let _=ut.get("WEBGL_lose_context");_&&_.restoreContext()},this.getPixelRatio=function(){return te},this.setPixelRatio=function(_){_!==void 0&&(te=_,this.setSize(rt,$,!1))},this.getSize=function(_){return _.set(rt,$)},this.setSize=function(_,C,Y=!0){if(Re.isPresenting){pe("WebGLRenderer: Can't change size while VR device is presenting.");return}rt=_,$=C,t.width=Math.floor(_*te),t.height=Math.floor(C*te),Y===!0&&(t.style.width=_+"px",t.style.height=C+"px"),M!==null&&M.setSize(t.width,t.height),this.setViewport(0,0,_,C)},this.getDrawingBufferSize=function(_){return _.set(rt*te,$*te).floor()},this.setDrawingBufferSize=function(_,C,Y){rt=_,$=C,te=Y,t.width=Math.floor(_*Y),t.height=Math.floor(C*Y),this.setViewport(0,0,_,C)},this.setEffects=function(_){if(y===mn){_e("WebGLRenderer: setEffects() requires outputBufferType set to HalfFloatType or FloatType.");return}if(_){for(let C=0;C<_.length;C++)if(_[C].isOutputPass===!0){pe("WebGLRenderer: OutputPass is not needed in setEffects(). Tone mapping and color space conversion are applied automatically.");break}}M.setEffects(_||[])},this.getCurrentViewport=function(_){return _.copy(se)},this.getViewport=function(_){return _.copy(ve)},this.setViewport=function(_,C,Y,G){_.isVector4?ve.set(_.x,_.y,_.z,_.w):ve.set(_,C,Y,G),x.viewport(se.copy(ve).multiplyScalar(te).round())},this.getScissor=function(_){return _.copy(Ke)},this.setScissor=function(_,C,Y,G){_.isVector4?Ke.set(_.x,_.y,_.z,_.w):Ke.set(_,C,Y,G),x.scissor(Ue.copy(Ke).multiplyScalar(te).round())},this.getScissorTest=function(){return Ot},this.setScissorTest=function(_){x.setScissorTest(Ot=_)},this.setOpaqueSort=function(_){Te=_},this.setTransparentSort=function(_){Oe=_},this.getClearColor=function(_){return _.copy(We.getClearColor())},this.setClearColor=function(){We.setClearColor(...arguments)},this.getClearAlpha=function(){return We.getClearAlpha()},this.setClearAlpha=function(){We.setClearAlpha(...arguments)},this.clear=function(_=!0,C=!0,Y=!0){let G=0;if(_){let V=!1;if(re!==null){let ye=re.texture.format;V=m.has(ye)}if(V){let ye=re.texture.type,Ae=p.has(ye),xe=We.getClearColor(),Ee=We.getClearAlpha(),Ce=xe.r,Ze=xe.g,Je=xe.b;Ae?(E[0]=Ce,E[1]=Ze,E[2]=Je,E[3]=Ee,I.clearBufferuiv(I.COLOR,0,E)):(w[0]=Ce,w[1]=Ze,w[2]=Je,w[3]=Ee,I.clearBufferiv(I.COLOR,0,w))}else G|=I.COLOR_BUFFER_BIT}C&&(G|=I.DEPTH_BUFFER_BIT,this.state.buffers.depth.setMask(!0)),Y&&(G|=I.STENCIL_BUFFER_BIT,this.state.buffers.stencil.setMask(4294967295)),G!==0&&I.clear(G)},this.clearColor=function(){this.clear(!0,!1,!1)},this.clearDepth=function(){this.clear(!1,!0,!1)},this.clearStencil=function(){this.clear(!1,!1,!0)},this.setNodesHandler=function(_){_.setRenderer(this),k=_},this.dispose=function(){t.removeEventListener("webglcontextlost",bt,!1),t.removeEventListener("webglcontextrestored",at,!1),t.removeEventListener("webglcontextcreationerror",On,!1),We.dispose(),fe.dispose(),ue.dispose(),z.dispose(),ae.dispose(),j.dispose(),be.dispose(),ie.dispose(),ce.dispose(),Re.dispose(),Re.removeEventListener("sessionstart",pp),Re.removeEventListener("sessionend",mp),rs.stop()};function bt(_){_.preventDefault(),hs("WebGLRenderer: Context Lost."),D=!0}function at(){hs("WebGLRenderer: Context Restored."),D=!1;let _=U.autoReset,C=Pe.enabled,Y=Pe.autoUpdate,G=Pe.needsUpdate,V=Pe.type;Le(),U.autoReset=_,Pe.enabled=C,Pe.autoUpdate=Y,Pe.needsUpdate=G,Pe.type=V}function On(_){_e("WebGLRenderer: A WebGL context could not be created. Reason: ",_.statusMessage)}function ri(_){let C=_.target;C.removeEventListener("dispose",ri),b0(C)}function b0(_){S0(_),z.remove(_)}function S0(_){let C=z.get(_).programs;C!==void 0&&(C.forEach(function(Y){ce.releaseProgram(Y)}),_.isShaderMaterial&&ce.releaseShaderCache(_))}this.renderBufferDirect=function(_,C,Y,G,V,ye){C===null&&(C=xn);let Ae=V.isMesh&&V.matrixWorld.determinantAffine()<0,xe=T0(_,C,Y,G,V);x.setMaterial(G,Ae);let Ee=Y.index,Ce=1;if(G.wireframe===!0){if(Ee=J.getWireframeAttribute(Y),Ee===void 0)return;Ce=2}let Ze=Y.drawRange,Je=Y.attributes.position,Me=Ze.start*Ce,lt=(Ze.start+Ze.count)*Ce;ye!==null&&(Me=Math.max(Me,ye.start*Ce),lt=Math.min(lt,(ye.start+ye.count)*Ce)),Ee!==null?(Me=Math.max(Me,0),lt=Math.min(lt,Ee.count)):Je!=null&&(Me=Math.max(Me,0),lt=Math.min(lt,Je.count));let Lt=lt-Me;if(Lt<0||Lt===1/0)return;be.setup(V,G,xe,Y,Ee);let vt,pt=he;if(Ee!==null&&(vt=le.get(Ee),pt=Q,pt.setIndex(vt)),V.isMesh)G.wireframe===!0?(x.setLineWidth(G.wireframeLinewidth*Ft()),pt.setMode(I.LINES)):pt.setMode(I.TRIANGLES);else if(V.isLine){let jt=G.linewidth;jt===void 0&&(jt=1),x.setLineWidth(jt*Ft()),V.isLineSegments?pt.setMode(I.LINES):V.isLineLoop?pt.setMode(I.LINE_LOOP):pt.setMode(I.LINE_STRIP)}else V.isPoints?pt.setMode(I.POINTS):V.isSprite&&pt.setMode(I.TRIANGLES);if(V.isBatchedMesh)if(ut.get("WEBGL_multi_draw"))pt.renderMultiDraw(V._multiDrawStarts,V._multiDrawCounts,V._multiDrawCount);else{let jt=V._multiDrawStarts,Se=V._multiDrawCounts,cn=V._multiDrawCount,tt=Ee?le.get(Ee).bytesPerElement:1,Bn=z.get(G).currentProgram.getUniforms();for(let oi=0;oi<cn;oi++)Bn.setValue(I,"_gl_DrawID",oi),pt.render(jt[oi]/tt,Se[oi])}else if(V.isInstancedMesh)pt.renderInstances(Me,Lt,V.count);else if(Y.isInstancedBufferGeometry){let jt=Y._maxInstanceCount!==void 0?Y._maxInstanceCount:1/0,Se=Math.min(Y.instanceCount,jt);pt.renderInstances(Me,Lt,Se)}else pt.render(Me,Lt)};function fp(_,C,Y,G){k!==null&&_.isNodeMaterial&&k.setObject(G,_),st===!0&&Be.setState(_,Y,!1),_.transparent===!0&&_.side===sn&&_.forceSinglePass===!1?(_.side=Dt,_.needsUpdate=!0,qa(_,C,G),_.side=Jn,_.needsUpdate=!0,qa(_,C,G),_.side=sn):qa(_,C,G)}this.compile=function(_,C,Y=null){Y===null&&(Y=_),k!==null&&k.renderStart(_,C,Y),A=ue.get(Y),A.init(C),b.push(A),Y.traverseVisible(function(V){V.isLight&&V.layers.test(C.layers)&&(A.pushLight(V),V.castShadow&&A.pushShadow(V))}),_!==Y&&_.traverseVisible(function(V){V.isLight&&V.layers.test(C.layers)&&(A.pushLight(V),V.castShadow&&A.pushShadow(V))}),A.setupLights(),k!==null&&k.updateLights(A.state.lightsArray),_t=this.localClippingEnabled,st=Be.init(this.clippingPlanes,_t),st===!0&&Be.setGlobalState(this.clippingPlanes,C),k!==null&&Pe.render(A.state.shadowsArray,Y,C);let G=new Set;return _.traverse(function(V){if(!(V.isMesh||V.isPoints||V.isLine||V.isSprite))return;let ye=V.material;if(ye)if(Array.isArray(ye))for(let Ae=0;Ae<ye.length;Ae++){let xe=ye[Ae];fp(xe,Y,C,V),G.add(xe)}else fp(ye,Y,C,V),G.add(ye)}),A=b.pop(),k!==null&&k.renderEnd(),G},this.compileAsync=function(_,C,Y=null){let G=this.compile(_,C,Y);return new Promise(V=>{function ye(){if(G.forEach(function(Ae){let Ee=z.get(Ae).currentProgram;(Ee===void 0||Ee.isReady())&&G.delete(Ae)}),G.size===0){V(_);return}setTimeout(ye,10)}ut.get("KHR_parallel_shader_compile")!==null?ye():setTimeout(ye,10)})};let Au=null;function v0(_){Au&&Au(_)}function pp(){rs.stop()}function mp(){rs.start()}let rs=new Rm;rs.setAnimationLoop(v0),typeof self<"u"&&rs.setContext(self),this.setAnimationLoop=function(_){Au=_,Re.setAnimationLoop(_),_===null?rs.stop():rs.start()},Re.addEventListener("sessionstart",pp),Re.addEventListener("sessionend",mp),this.render=function(_,C){if(C!==void 0&&C.isCamera!==!0){_e("WebGLRenderer.render: camera is not an instance of THREE.Camera.");return}if(D===!0)return;k!==null&&k.renderStart(_,C);let Y=Re.enabled===!0&&Re.isPresenting===!0,G=M!==null&&(re===null||Y)&&M.begin(B,re);if(_.matrixWorldAutoUpdate===!0&&_.updateMatrixWorld(),C.parent===null&&C.matrixWorldAutoUpdate===!0&&C.updateMatrixWorld(),Re.enabled===!0&&Re.isPresenting===!0&&(M===null||M.isCompositing()===!1)&&(Re.cameraAutoUpdate===!0&&Re.updateCamera(C),C=Re.getCamera()),_.isScene===!0&&_.onBeforeRender(B,_,C,re),A=ue.get(_,b.length),A.init(C),A.state.textureUnits=Z.getTextureUnits(),b.push(A),Qe.multiplyMatrices(C.projectionMatrix,C.matrixWorldInverse),$e.setFromProjectionMatrix(Qe,hn,C.reversedDepth),_t=this.localClippingEnabled,st=Be.init(this.clippingPlanes,_t),v=fe.get(_,R.length),v.init(),R.push(v),Re.enabled===!0&&Re.isPresenting===!0){let Ae=B.xr.getDepthSensingMesh();Ae!==null&&Tu(Ae,C,-1/0,B.sortObjects)}Tu(_,C,0,B.sortObjects),v.finish(),k!==null&&k.updateLights(A.state.lightsArray),B.sortObjects===!0&&v.sort(Te,Oe),Mt=Re.enabled===!1||Re.isPresenting===!1||Re.hasDepthSensing()===!1,Mt&&We.addToRenderList(v,_),this.info.render.frame++,this.info.autoReset===!0&&this.info.reset(),st===!0&&Be.beginShadows();let V=A.state.shadowsArray;if(Pe.render(V,_,C),st===!0&&Be.endShadows(),(G&&M.hasRenderPass())===!1){let Ae=v.opaque,xe=v.transmissive;if(A.setupLights(),C.isArrayCamera){let Ee=C.cameras;if(xe.length>0)for(let Ce=0,Ze=Ee.length;Ce<Ze;Ce++){let Je=Ee[Ce];xp(Ae,xe,_,Je)}Mt&&We.render(_);for(let Ce=0,Ze=Ee.length;Ce<Ze;Ce++){let Je=Ee[Ce];gp(v,_,Je,Je.viewport)}}else xe.length>0&&xp(Ae,xe,_,C),Mt&&We.render(_),gp(v,_,C)}re!==null&&X===0&&(Z.updateMultisampleRenderTarget(re),Z.updateRenderTargetMipmap(re)),G&&M.end(B),_.isScene===!0&&_.onAfterRender(B,_,C),be.resetDefaultState(),K=-1,ne=null,b.pop(),b.length>0?(A=b[b.length-1],Z.setTextureUnits(A.state.textureUnits),st===!0&&Be.setGlobalState(B.clippingPlanes,A.state.camera)):A=null,R.pop(),R.length>0?v=R[R.length-1]:v=null,k!==null&&k.renderEnd()};function Tu(_,C,Y,G){if(_.visible===!1)return;if(_.layers.test(C.layers)){if(_.isGroup)Y=_.renderOrder;else if(_.isLOD)_.autoUpdate===!0&&_.update(C);else if(_.isLightProbeGrid)A.pushLightProbeGrid(_);else if(_.isLight)A.pushLight(_),_.castShadow&&A.pushShadow(_);else if(_.isSprite){if(!_.frustumCulled||_.intersectsFrustum($e)){G&&Yt.setFromMatrixPosition(_.matrixWorld).applyMatrix4(Qe);let Ae=j.update(_),xe=_.material;xe.visible&&v.push(_,Ae,xe,Y,Yt.z,null,C)}}else if((_.isMesh||_.isLine||_.isPoints)&&(!_.frustumCulled||_.intersectsFrustum($e))){let Ae=j.update(_),xe=_.material;if(G&&(_.boundingSphere!==void 0?(_.boundingSphere===null&&_.computeBoundingSphere(),Yt.copy(_.boundingSphere.center)):(Ae.boundingSphere===null&&Ae.computeBoundingSphere(),Yt.copy(Ae.boundingSphere.center)),Yt.applyMatrix4(_.matrixWorld).applyMatrix4(Qe)),Array.isArray(xe)){let Ee=Ae.groups;for(let Ce=0,Ze=Ee.length;Ce<Ze;Ce++){let Je=Ee[Ce],Me=xe[Je.materialIndex];Me&&Me.visible&&v.push(_,Ae,Me,Y,Yt.z,Je,C)}}else xe.visible&&v.push(_,Ae,xe,Y,Yt.z,null,C)}}let ye=_.children;for(let Ae=0,xe=ye.length;Ae<xe;Ae++)Tu(ye[Ae],C,Y,G)}function gp(_,C,Y,G){let{opaque:V,transmissive:ye,transparent:Ae}=_;A.setupLightsView(Y),st===!0&&Be.setGlobalState(B.clippingPlanes,Y),G&&x.viewport(se.copy(G)),V.length>0&&Ja(V,C,Y),ye.length>0&&Ja(ye,C,Y),Ae.length>0&&Ja(Ae,C,Y),x.buffers.depth.setTest(!0),x.buffers.depth.setMask(!0),x.buffers.color.setMask(!0),x.setPolygonOffset(!1)}function xp(_,C,Y,G){if((Y.isScene===!0?Y.overrideMaterial:null)!==null)return;if(A.state.transmissionRenderTarget[G.id]===void 0){let Me=ut.has("EXT_color_buffer_half_float")||ut.has("EXT_color_buffer_float");A.state.transmissionRenderTarget[G.id]=new _n(1,1,{generateMipmaps:!0,type:Me?Sn:mn,minFilter:di,samples:Math.max(4,T.samples),stencilBuffer:r,resolveDepthBuffer:!1,resolveStencilBuffer:!1,storeMultisampledDepthBuffer:!1,storeMultisampledStencilBuffer:!1,colorSpace:Ve.workingColorSpace})}let ye=A.state.transmissionRenderTarget[G.id],Ae=G.viewport||se;ye.setSize(Ae.z*B.transmissionResolutionScale,Ae.w*B.transmissionResolutionScale);let xe=B.getRenderTarget(),Ee=B.getActiveCubeFace(),Ce=B.getActiveMipmapLevel();B.setRenderTarget(ye),B.getClearColor(yt),je=B.getClearAlpha(),je<1&&B.setClearColor(16777215,.5),B.clear(),Mt&&We.render(Y);let Ze=B.toneMapping;B.toneMapping=pn;let Je=G.viewport;if(G.viewport!==void 0&&(G.viewport=void 0),A.setupLightsView(G),st===!0&&Be.setGlobalState(B.clippingPlanes,G),Ja(_,Y,G),Z.updateMultisampleRenderTarget(ye),Z.updateRenderTargetMipmap(ye),ut.has("WEBGL_multisampled_render_to_texture")===!1){let Me=!1;for(let lt=0,Lt=C.length;lt<Lt;lt++){let vt=C[lt],{object:pt,geometry:jt,material:Se,group:cn}=vt;if(Se.side===sn&&pt.layers.test(G.layers)){let tt=Se.side;Se.side=Dt,Se.needsUpdate=!0,yp(pt,Y,G,jt,Se,cn),Se.side=tt,Se.needsUpdate=!0,Me=!0}}Me===!0&&(Z.updateMultisampleRenderTarget(ye),Z.updateRenderTargetMipmap(ye))}B.setRenderTarget(xe,Ee,Ce),B.setClearColor(yt,je),Je!==void 0&&(G.viewport=Je),B.toneMapping=Ze}function Ja(_,C,Y){let G=C.isScene===!0?C.overrideMaterial:null;for(let V=0,ye=_.length;V<ye;V++){let Ae=_[V],{object:xe,geometry:Ee,group:Ce}=Ae,Ze=Ae.material;Ze.allowOverride===!0&&G!==null&&(Ze=G),xe.layers.test(Y.layers)&&yp(xe,C,Y,Ee,Ze,Ce)}}function yp(_,C,Y,G,V,ye){k!==null&&V.isNodeMaterial&&k.setObject(_,V),_.onBeforeRender(B,C,Y,G,V,ye),_.modelViewMatrix.multiplyMatrices(Y.matrixWorldInverse,_.matrixWorld),_.normalMatrix.getNormalMatrix(_.modelViewMatrix),V.onBeforeRender(B,C,Y,G,_,ye),V.transparent===!0&&V.side===sn&&V.forceSinglePass===!1?(V.side=Dt,V.needsUpdate=!0,B.renderBufferDirect(Y,C,G,V,_,ye),V.side=Jn,V.needsUpdate=!0,B.renderBufferDirect(Y,C,G,V,_,ye),V.side=sn):B.renderBufferDirect(Y,C,G,V,_,ye),_.onAfterRender(B,C,Y,G,V,ye)}function qa(_,C,Y){C.isScene!==!0&&(C=xn);let G=z.get(_),V=A.state.lights,ye=A.state.shadowsArray,Ae=V.state.version,xe=ce.getParameters(_,V.state,ye,C,Y,A.state.lightProbeGridArray),Ee=ce.getProgramCacheKey(xe),Ce=G.programs;G.environment=_.isMeshStandardMaterial||_.isMeshLambertMaterial||_.isMeshPhongMaterial?C.environment:null,G.fog=C.fog;let Ze=_.isMeshStandardMaterial||_.isMeshLambertMaterial&&!_.envMap||_.isMeshPhongMaterial&&!_.envMap;G.envMap=ae.get(_.envMap||G.environment,Ze),G.envMapRotation=G.environment!==null&&_.envMap===null?C.environmentRotation:_.envMapRotation,Ce===void 0&&(_.addEventListener("dispose",ri),Ce=new Map,G.programs=Ce);let Je=Ce.get(Ee);if(Je!==void 0){if(G.currentProgram===Je&&G.lightsStateVersion===Ae)return bp(_,xe),Je}else xe.uniforms=ce.getUniforms(_),k!==null&&_.isNodeMaterial&&k.build(_,Y,xe),_.onBeforeCompile(xe,B),Je=ce.acquireProgram(xe,Ee),Ce.set(Ee,Je),G.uniforms=xe.uniforms;let Me=G.uniforms;return(!_.isShaderMaterial&&!_.isRawShaderMaterial||_.clipping===!0)&&(Me.clippingPlanes=Be.uniform),bp(_,xe),G.needsLights=M0(_),G.lightsStateVersion=Ae,G.needsLights&&(Me.ambientLightColor.value=V.state.ambient,Me.lightProbe.value=V.state.probe,Me.sunLights.value=V.state.sun,Me.sunLightShadows.value=V.state.sunShadow,Me.directionalLights.value=V.state.directional,Me.directionalLightShadows.value=V.state.directionalShadow,Me.spotLights.value=V.state.spot,Me.spotLightShadows.value=V.state.spotShadow,Me.rectAreaLights.value=V.state.rectArea,Me.ltc_1.value=V.state.rectAreaLTC1,Me.ltc_2.value=V.state.rectAreaLTC2,Me.pointLights.value=V.state.point,Me.pointLightShadows.value=V.state.pointShadow,Me.hemisphereLights.value=V.state.hemi,Me.sunShadowMatrix.value=V.state.sunShadowMatrix,Me.sunShadowCascade.value=V.state.sunShadowCascade,Me.directionalShadowMatrix.value=V.state.directionalShadowMatrix,Me.spotLightMatrix.value=V.state.spotLightMatrix,Me.spotLightMap.value=V.state.spotLightMap,Me.pointShadowMatrix.value=V.state.pointShadowMatrix),G.lightProbeGrid=A.state.lightProbeGridArray.length>0,G.currentProgram=Je,G.uniformsList=null,Je}function _p(_){if(_.uniformsList===null){let C=_.currentProgram.getUniforms();_.uniformsList=Pr.seqWithValue(C.seq,_.uniforms)}return _.uniformsList}function bp(_,C){let Y=z.get(_);Y.outputColorSpace=C.outputColorSpace,Y.batching=C.batching,Y.batchingColor=C.batchingColor,Y.instancing=C.instancing,Y.instancingColor=C.instancingColor,Y.instancingMorph=C.instancingMorph,Y.skinning=C.skinning,Y.morphTargets=C.morphTargets,Y.morphNormals=C.morphNormals,Y.morphColors=C.morphColors,Y.morphTargetsCount=C.morphTargetsCount,Y.numClippingPlanes=C.numClippingPlanes,Y.numIntersection=C.numClipIntersection,Y.vertexAlphas=C.vertexAlphas,Y.vertexTangents=C.vertexTangents,Y.toneMapping=C.toneMapping}function A0(_,C){if(_.length===0)return null;if(_.length===1)return _[0].texture!==null?_[0]:null;S.setFromMatrixPosition(C.matrixWorld);for(let Y=0,G=_.length;Y<G;Y++){let V=_[Y];if(V.texture!==null&&V.boundingBox.containsPoint(S))return V}return null}function T0(_,C,Y,G,V){C.isScene!==!0&&(C=xn),Z.resetTextureUnits();let ye=C.fog,Ae=G.isMeshStandardMaterial||G.isMeshLambertMaterial||G.isMeshPhongMaterial?C.environment:null,xe=re===null?B.outputColorSpace:re.isXRRenderTarget===!0?re.texture.colorSpace:Ve.workingColorSpace,Ee=G.isMeshStandardMaterial||G.isMeshLambertMaterial&&!G.envMap||G.isMeshPhongMaterial&&!G.envMap,Ce=ae.get(G.envMap||Ae,Ee),Ze=G.vertexColors===!0&&!!Y.attributes.color&&Y.attributes.color.itemSize===4,Je=!!Y.attributes.tangent&&(!!G.normalMap||G.anisotropy>0),Me=!!Y.morphAttributes.position,lt=!!Y.morphAttributes.normal,Lt=!!Y.morphAttributes.color,vt=pn;G.toneMapped&&(re===null||re.isXRRenderTarget===!0)&&(vt=B.toneMapping);let pt=Y.morphAttributes.position||Y.morphAttributes.normal||Y.morphAttributes.color,jt=pt!==void 0?pt.length:0,Se=z.get(G),cn=A.state.lights;if(st===!0&&(_t===!0||_!==ne)){let St=_===ne&&G.id===K;Be.setState(G,_,St)}let tt=!1;G.version===Se.__version?(Se.needsLights&&Se.lightsStateVersion!==cn.state.version||Se.outputColorSpace!==xe||V.isBatchedMesh&&Se.batching===!1||!V.isBatchedMesh&&Se.batching===!0||V.isBatchedMesh&&Se.batchingColor===!0&&V._colorsTexture===null||V.isBatchedMesh&&Se.batchingColor===!1&&V._colorsTexture!==null||V.isInstancedMesh&&Se.instancing===!1||!V.isInstancedMesh&&Se.instancing===!0||V.isSkinnedMesh&&Se.skinning===!1||!V.isSkinnedMesh&&Se.skinning===!0||V.isInstancedMesh&&Se.instancingColor===!0&&V.instanceColor===null||V.isInstancedMesh&&Se.instancingColor===!1&&V.instanceColor!==null||V.isInstancedMesh&&Se.instancingMorph===!0&&V.morphTexture===null||V.isInstancedMesh&&Se.instancingMorph===!1&&V.morphTexture!==null||Se.envMap!==Ce||G.fog===!0&&Se.fog!==ye||Se.numClippingPlanes!==void 0&&(Se.numClippingPlanes!==Be.numPlanes||Se.numIntersection!==Be.numIntersection)||Se.vertexAlphas!==Ze||Se.vertexTangents!==Je||Se.morphTargets!==Me||Se.morphNormals!==lt||Se.morphColors!==Lt||Se.toneMapping!==vt||Se.morphTargetsCount!==jt||!!Se.lightProbeGrid!=A.state.lightProbeGridArray.length>0)&&(tt=!0):(tt=!0,Se.__version=G.version);let Bn=Se.currentProgram;tt===!0&&(Bn=qa(G,C,V),k&&G.isNodeMaterial&&k.onUpdateProgram(G,Bn,Se));let oi=!1,Ui=!1,Hs=!1,ft=Bn.getUniforms(),It=Se.uniforms;if(x.useProgram(Bn.program)&&(oi=!0,Ui=!0,Hs=!0),G.id!==K&&(K=G.id,Ui=!0),Se.needsLights){let St=A0(A.state.lightProbeGridArray,V);Se.lightProbeGrid!==St&&(Se.lightProbeGrid=St,Ui=!0)}if(oi||ne!==_){x.buffers.depth.getReversed()&&_.reversedDepth!==!0&&(_._reversedDepth=!0,_.updateProjectionMatrix()),ft.setValue(I,"projectionMatrix",_.projectionMatrix),ft.setValue(I,"viewMatrix",_.matrixWorldInverse);let Pi=ft.map.cameraPosition;Pi!==void 0&&Pi.setValue(I,Tt.setFromMatrixPosition(_.matrixWorld)),T.logarithmicDepthBuffer&&ft.setValue(I,"logDepthBufFC",2/(Math.log(_.far+1)/Math.LN2)),(G.isMeshPhongMaterial||G.isMeshToonMaterial||G.isMeshLambertMaterial||G.isMeshBasicMaterial||G.isMeshStandardMaterial||G.isShaderMaterial)&&ft.setValue(I,"isOrthographic",_.isOrthographicCamera===!0),ne!==_&&(ne=_,Ui=!0,Hs=!0)}if(Se.needsLights&&(cn.state.sunShadowMap.length>0&&ft.setValue(I,"sunShadowMap",cn.state.sunShadowMap,Z),cn.state.directionalShadowMap.length>0&&ft.setValue(I,"directionalShadowMap",cn.state.directionalShadowMap,Z),cn.state.spotShadowMap.length>0&&ft.setValue(I,"spotShadowMap",cn.state.spotShadowMap,Z),cn.state.pointShadowMap.length>0&&ft.setValue(I,"pointShadowMap",cn.state.pointShadowMap,Z)),V.isSkinnedMesh){ft.setOptional(I,V,"bindMatrix"),ft.setOptional(I,V,"bindMatrixInverse");let St=V.skeleton;St&&(St.boneTexture===null&&St.computeBoneTexture(),ft.setValue(I,"boneTexture",St.boneTexture,Z))}V.isBatchedMesh&&(ft.setOptional(I,V,"batchingTexture"),ft.setValue(I,"batchingTexture",V._matricesTexture,Z),ft.setOptional(I,V,"batchingIdTexture"),ft.setValue(I,"batchingIdTexture",V._indirectTexture,Z),ft.setOptional(I,V,"batchingColorTexture"),V._colorsTexture!==null&&ft.setValue(I,"batchingColorTexture",V._colorsTexture,Z));let Di=Y.morphAttributes;if((Di.position!==void 0||Di.normal!==void 0||Di.color!==void 0)&&N.update(V,Y,Bn),(Ui||Se.receiveShadow!==V.receiveShadow)&&(Se.receiveShadow=V.receiveShadow,ft.setValue(I,"receiveShadow",V.receiveShadow)),(G.isMeshStandardMaterial||G.isMeshLambertMaterial||G.isMeshPhongMaterial)&&G.envMap===null&&C.environment!==null&&(It.envMapIntensity.value=C.environmentIntensity),It.dfgLUT!==void 0&&(It.dfgLUT.value=sT()),Ui){if(ft.setValue(I,"toneMappingExposure",B.toneMappingExposure),Se.needsLights&&E0(It,Hs),ye&&G.fog===!0&&Ie.refreshFogUniforms(It,ye),Ie.refreshMaterialUniforms(It,G,te,$,A.state.transmissionRenderTarget[_.id]),Se.needsLights&&Se.lightProbeGrid){let St=Se.lightProbeGrid;It.probesSH.value=St.texture,It.probesMin.value.copy(St.boundingBox.min),It.probesMax.value.copy(St.boundingBox.max),It.probesResolution.value.copy(St.resolution)}Pr.upload(I,_p(Se),It,Z)}if(G.isShaderMaterial&&G.uniformsNeedUpdate===!0&&(Pr.upload(I,_p(Se),It,Z),G.uniformsNeedUpdate=!1),G.isSpriteMaterial&&ft.setValue(I,"center",V.center),ft.setValue(I,"modelViewMatrix",V.modelViewMatrix),ft.setValue(I,"normalMatrix",V.normalMatrix),ft.setValue(I,"modelMatrix",V.matrixWorld),G.uniformsGroups!==void 0){let St=G.uniformsGroups;for(let Pi=0,Ys=St.length;Pi<Ys;Pi++){let vp=St[Pi];ie.update(vp,Bn),ie.bind(vp,Bn)}}return Bn}function E0(_,C){_.ambientLightColor.needsUpdate=C,_.lightProbe.needsUpdate=C,_.sunLights.needsUpdate=C,_.sunLightShadows.needsUpdate=C,_.directionalLights.needsUpdate=C,_.directionalLightShadows.needsUpdate=C,_.pointLights.needsUpdate=C,_.pointLightShadows.needsUpdate=C,_.spotLights.needsUpdate=C,_.spotLightShadows.needsUpdate=C,_.rectAreaLights.needsUpdate=C,_.hemisphereLights.needsUpdate=C}function M0(_){return _.isMeshLambertMaterial||_.isMeshToonMaterial||_.isMeshPhongMaterial||_.isMeshStandardMaterial||_.isShadowMaterial||_.isShaderMaterial&&_.lights===!0}this.getActiveCubeFace=function(){return q},this.getActiveMipmapLevel=function(){return X},this.getRenderTarget=function(){return re},this.setRenderTargetTextures=function(_,C,Y){let G=z.get(_);G.__autoAllocateDepthBuffer=_.resolveDepthBuffer===!1,G.__autoAllocateDepthBuffer===!1&&(G.__useRenderToTexture=!1),z.get(_.texture).__webglTexture=C,z.get(_.depthTexture).__webglTexture=G.__autoAllocateDepthBuffer?void 0:Y,G.__hasExternalTextures=!0},this.setRenderTargetFramebuffer=function(_,C){let Y=z.get(_);Y.__webglFramebuffer=C,Y.__useDefaultFramebuffer=C===void 0},this.setRenderTarget=function(_,C=0,Y=0){re=_,q=C,X=Y;let G=null,V=!1,ye=!1;if(_){let xe=z.get(_);if(xe.__useDefaultFramebuffer!==void 0){x.bindFramebuffer(I.FRAMEBUFFER,xe.__webglFramebuffer),se.copy(_.viewport),Ue.copy(_.scissor),Fe=_.scissorTest,x.viewport(se),x.scissor(Ue),x.setScissorTest(Fe),K=-1;return}else if(xe.__webglFramebuffer===void 0)Z.setupRenderTarget(_);else if(xe.__hasExternalTextures)Z.rebindTextures(_,z.get(_.texture).__webglTexture,z.get(_.depthTexture).__webglTexture);else if(_.depthBuffer){let Ze=_.depthTexture;if(xe.__boundDepthTexture!==Ze){if(Ze!==null&&z.has(Ze)&&(_.width!==Ze.image.width||_.height!==Ze.image.height))throw new Error("THREE.WebGLRenderer: Attached DepthTexture is initialized to the incorrect size.");Z.setupDepthRenderbuffer(_)}}let Ee=_.texture;(Ee.isData3DTexture||Ee.isDataArrayTexture||Ee.isCompressedArrayTexture)&&(ye=!0);let Ce=z.get(_).__webglFramebuffer;_.isWebGLCubeRenderTarget?(Array.isArray(Ce[C])?G=Ce[C][Y]:G=Ce[C],V=!0):_.samples>0&&Z.useMultisampledRTT(_)===!1?G=z.get(_).__webglMultisampledFramebuffer:Array.isArray(Ce)?G=Ce[Y]:G=Ce,se.copy(_.viewport),Ue.copy(_.scissor),Fe=_.scissorTest}else se.copy(ve).multiplyScalar(te).floor(),Ue.copy(Ke).multiplyScalar(te).floor(),Fe=Ot;if(Y!==0&&(G=H),x.bindFramebuffer(I.FRAMEBUFFER,G)&&x.drawBuffers(_,G),x.viewport(se),x.scissor(Ue),x.setScissorTest(Fe),V){let xe=z.get(_.texture);I.framebufferTexture2D(I.FRAMEBUFFER,I.COLOR_ATTACHMENT0,I.TEXTURE_CUBE_MAP_POSITIVE_X+C,xe.__webglTexture,Y)}else if(ye){let xe=C;for(let Ee=0;Ee<_.textures.length;Ee++){let Ce=z.get(_.textures[Ee]);I.framebufferTextureLayer(I.FRAMEBUFFER,I.COLOR_ATTACHMENT0+Ee,Ce.__webglTexture,Y,xe)}}else if(_!==null&&Y!==0){let xe=z.get(_.texture);I.framebufferTexture2D(I.FRAMEBUFFER,I.COLOR_ATTACHMENT0,I.TEXTURE_2D,xe.__webglTexture,Y)}K=-1};function Sp(_){let C=z.get(_);return(C.__readFormat!==_.format||C.__readType!==_.type)&&(C.__readFormat=_.format,C.__readType=_.type,C.__formatReadable=T.textureFormatReadable(_.format),C.__typeReadable=T.textureTypeReadable(_.type)),C}this.readRenderTargetPixels=function(_,C,Y,G,V,ye,Ae,xe=0){if(!(_&&_.isWebGLRenderTarget)){_e("WebGLRenderer.readRenderTargetPixels: renderTarget is not THREE.WebGLRenderTarget.");return}let Ee=z.get(_).__webglFramebuffer;if(_.isWebGLCubeRenderTarget&&Ae!==void 0&&(Ee=Ee[Ae]),Ee){x.bindFramebuffer(I.FRAMEBUFFER,Ee);try{let Ce=_.textures[xe],Ze=Ce.format,Je=Ce.type;_.textures.length>1&&I.readBuffer(I.COLOR_ATTACHMENT0+xe);let Me=Sp(Ce);if(Me.__formatReadable===!1){_e("WebGLRenderer.readRenderTargetPixels: renderTarget is not in RGBA or implementation defined format.");return}if(Me.__typeReadable===!1){_e("WebGLRenderer.readRenderTargetPixels: renderTarget is not in UnsignedByteType or implementation defined type.");return}C>=0&&C<=_.width-G&&Y>=0&&Y<=_.height-V&&I.readPixels(C,Y,G,V,de.convert(Ze),de.convert(Je),ye)}finally{let Ce=re!==null?z.get(re).__webglFramebuffer:null;x.bindFramebuffer(I.FRAMEBUFFER,Ce)}}},this.readRenderTargetPixelsAsync=async function(_,C,Y,G,V,ye,Ae,xe=0){if(!(_&&_.isWebGLRenderTarget))throw new Error("THREE.WebGLRenderer.readRenderTargetPixels: renderTarget is not THREE.WebGLRenderTarget.");let Ee=z.get(_).__webglFramebuffer;if(_.isWebGLCubeRenderTarget&&Ae!==void 0&&(Ee=Ee[Ae]),Ee)if(C>=0&&C<=_.width-G&&Y>=0&&Y<=_.height-V){x.bindFramebuffer(I.FRAMEBUFFER,Ee);let Ce=_.textures[xe],Ze=Ce.format,Je=Ce.type;_.textures.length>1&&I.readBuffer(I.COLOR_ATTACHMENT0+xe);let Me=Sp(Ce);if(Me.__formatReadable===!1)throw new Error("THREE.WebGLRenderer.readRenderTargetPixelsAsync: renderTarget is not in RGBA or implementation defined format.");if(Me.__typeReadable===!1)throw new Error("THREE.WebGLRenderer.readRenderTargetPixelsAsync: renderTarget is not in UnsignedByteType or implementation defined type.");let lt=I.createBuffer();I.bindBuffer(I.PIXEL_PACK_BUFFER,lt),I.bufferData(I.PIXEL_PACK_BUFFER,ye.byteLength,I.STREAM_READ),I.readPixels(C,Y,G,V,de.convert(Ze),de.convert(Je),0),I.bindBuffer(I.PIXEL_PACK_BUFFER,null);let Lt=re!==null?z.get(re).__webglFramebuffer:null;x.bindFramebuffer(I.FRAMEBUFFER,Lt);let vt=I.fenceSync(I.SYNC_GPU_COMMANDS_COMPLETE,0);return I.flush(),await em(I,vt,4),I.bindBuffer(I.PIXEL_PACK_BUFFER,lt),I.getBufferSubData(I.PIXEL_PACK_BUFFER,0,ye),I.bindBuffer(I.PIXEL_PACK_BUFFER,null),I.deleteBuffer(lt),I.deleteSync(vt),ye}else throw new Error("THREE.WebGLRenderer.readRenderTargetPixelsAsync: requested read bounds are out of range.")},this.copyFramebufferToTexture=function(_,C=null,Y=0){let G=Math.pow(2,-Y),V=Math.floor(_.image.width*G),ye=Math.floor(_.image.height*G),Ae=C!==null?C.x:0,xe=C!==null?C.y:0;Z.setTexture2D(_,0),I.copyTexSubImage2D(I.TEXTURE_2D,Y,0,0,Ae,xe,V,ye),x.unbindTexture()},this.copyTextureToTexture=function(_,C,Y=null,G=null,V=0,ye=0){let Ae,xe,Ee,Ce,Ze,Je,Me,lt,Lt,vt=_.isCompressedTexture?_.mipmaps[ye]:_.image;if(Y!==null)Ae=Y.max.x-Y.min.x,xe=Y.max.y-Y.min.y,Ee=Y.isBox3?Y.max.z-Y.min.z:1,Ce=Y.min.x,Ze=Y.min.y,Je=Y.isBox3?Y.min.z:0;else{let It=Math.pow(2,-V);Ae=Math.floor(vt.width*It),xe=Math.floor(vt.height*It),_.isDataArrayTexture?Ee=vt.depth:_.isData3DTexture?Ee=Math.floor(vt.depth*It):Ee=1,Ce=0,Ze=0,Je=0}G!==null?(Me=G.x,lt=G.y,Lt=G.z):(Me=0,lt=0,Lt=0);let pt=de.convert(C.format),jt=de.convert(C.type),Se;C.isData3DTexture?(Z.setTexture3D(C,0),Se=I.TEXTURE_3D):C.isDataArrayTexture||C.isCompressedArrayTexture?(Z.setTexture2DArray(C,0),Se=I.TEXTURE_2D_ARRAY):(Z.setTexture2D(C,0),Se=I.TEXTURE_2D),x.activeTexture(I.TEXTURE0),x.pixelStorei(I.UNPACK_FLIP_Y_WEBGL,C.flipY),x.pixelStorei(I.UNPACK_PREMULTIPLY_ALPHA_WEBGL,C.premultiplyAlpha),x.pixelStorei(I.UNPACK_ALIGNMENT,C.unpackAlignment);let cn=x.getParameter(I.UNPACK_ROW_LENGTH),tt=x.getParameter(I.UNPACK_IMAGE_HEIGHT),Bn=x.getParameter(I.UNPACK_SKIP_PIXELS),oi=x.getParameter(I.UNPACK_SKIP_ROWS),Ui=x.getParameter(I.UNPACK_SKIP_IMAGES);x.pixelStorei(I.UNPACK_ROW_LENGTH,vt.width),x.pixelStorei(I.UNPACK_IMAGE_HEIGHT,vt.height),x.pixelStorei(I.UNPACK_SKIP_PIXELS,Ce),x.pixelStorei(I.UNPACK_SKIP_ROWS,Ze),x.pixelStorei(I.UNPACK_SKIP_IMAGES,Je);let Hs=_.isDataArrayTexture||_.isData3DTexture,ft=C.isDataArrayTexture||C.isData3DTexture;if(_.isDepthTexture){let It=z.get(_),Di=z.get(C),St=z.get(It.__renderTarget),Pi=z.get(Di.__renderTarget);x.bindFramebuffer(I.READ_FRAMEBUFFER,St.__webglFramebuffer),x.bindFramebuffer(I.DRAW_FRAMEBUFFER,Pi.__webglFramebuffer);for(let Ys=0;Ys<Ee;Ys++)Hs&&(I.framebufferTextureLayer(I.READ_FRAMEBUFFER,I.COLOR_ATTACHMENT0,z.get(_).__webglTexture,V,Je+Ys),I.framebufferTextureLayer(I.DRAW_FRAMEBUFFER,I.COLOR_ATTACHMENT0,z.get(C).__webglTexture,ye,Lt+Ys)),I.blitFramebuffer(Ce,Ze,Ae,xe,Me,lt,Ae,xe,I.DEPTH_BUFFER_BIT,I.NEAREST);x.bindFramebuffer(I.READ_FRAMEBUFFER,null),x.bindFramebuffer(I.DRAW_FRAMEBUFFER,null)}else if(V!==0||_.isRenderTargetTexture||z.has(_)){let It=z.get(_),Di=z.get(C);x.bindFramebuffer(I.READ_FRAMEBUFFER,F),x.bindFramebuffer(I.DRAW_FRAMEBUFFER,O);for(let St=0;St<Ee;St++)Hs?I.framebufferTextureLayer(I.READ_FRAMEBUFFER,I.COLOR_ATTACHMENT0,It.__webglTexture,V,Je+St):I.framebufferTexture2D(I.READ_FRAMEBUFFER,I.COLOR_ATTACHMENT0,I.TEXTURE_2D,It.__webglTexture,V),ft?I.framebufferTextureLayer(I.DRAW_FRAMEBUFFER,I.COLOR_ATTACHMENT0,Di.__webglTexture,ye,Lt+St):I.framebufferTexture2D(I.DRAW_FRAMEBUFFER,I.COLOR_ATTACHMENT0,I.TEXTURE_2D,Di.__webglTexture,ye),V!==0?I.blitFramebuffer(Ce,Ze,Ae,xe,Me,lt,Ae,xe,I.COLOR_BUFFER_BIT,I.NEAREST):ft?I.copyTexSubImage3D(Se,ye,Me,lt,Lt+St,Ce,Ze,Ae,xe):I.copyTexSubImage2D(Se,ye,Me,lt,Ce,Ze,Ae,xe);x.bindFramebuffer(I.READ_FRAMEBUFFER,null),x.bindFramebuffer(I.DRAW_FRAMEBUFFER,null)}else ft?_.isDataTexture||_.isData3DTexture?I.texSubImage3D(Se,ye,Me,lt,Lt,Ae,xe,Ee,pt,jt,vt.data):C.isCompressedArrayTexture?I.compressedTexSubImage3D(Se,ye,Me,lt,Lt,Ae,xe,Ee,pt,vt.data):I.texSubImage3D(Se,ye,Me,lt,Lt,Ae,xe,Ee,pt,jt,vt):_.isDataTexture?I.texSubImage2D(I.TEXTURE_2D,ye,Me,lt,Ae,xe,pt,jt,vt.data):_.isCompressedTexture?I.compressedTexSubImage2D(I.TEXTURE_2D,ye,Me,lt,vt.width,vt.height,pt,vt.data):I.texSubImage2D(I.TEXTURE_2D,ye,Me,lt,Ae,xe,pt,jt,vt);x.pixelStorei(I.UNPACK_ROW_LENGTH,cn),x.pixelStorei(I.UNPACK_IMAGE_HEIGHT,tt),x.pixelStorei(I.UNPACK_SKIP_PIXELS,Bn),x.pixelStorei(I.UNPACK_SKIP_ROWS,oi),x.pixelStorei(I.UNPACK_SKIP_IMAGES,Ui),ye===0&&C.generateMipmaps&&I.generateMipmap(Se),x.unbindTexture()},this.initRenderTarget=function(_){z.get(_).__webglFramebuffer===void 0&&Z.setupRenderTarget(_)},this.initTexture=function(_){_.isCubeTexture?Z.setTextureCube(_,0):_.isData3DTexture?Z.setTexture3D(_,0):_.isDataArrayTexture||_.isCompressedArrayTexture?Z.setTexture2DArray(_,0):Z.setTexture2D(_,0),x.unbindTexture()},this.resetState=function(){q=0,X=0,re=null,x.reset(),be.reset()},typeof __THREE_DEVTOOLS__<"u"&&__THREE_DEVTOOLS__.dispatchEvent(new CustomEvent("observe",{detail:this}))}get coordinateSystem(){return hn}get outputColorSpace(){return this._outputColorSpace}set outputColorSpace(e){this._outputColorSpace=e;let t=this.getContext();t.drawingBufferColorSpace=Ve._getDrawingBufferColorSpace(e),t.unpackColorSpace=Ve._getUnpackColorSpace()}};function jh(n,e){return n!==null&&typeof n=="object"&&"$typeName"in n&&typeof n.$typeName=="string"?e===void 0?!0:e.typeName===n.$typeName:!1}var oe;(function(n){n[n.DOUBLE=1]="DOUBLE",n[n.FLOAT=2]="FLOAT",n[n.INT64=3]="INT64",n[n.UINT64=4]="UINT64",n[n.INT32=5]="INT32",n[n.FIXED64=6]="FIXED64",n[n.FIXED32=7]="FIXED32",n[n.BOOL=8]="BOOL",n[n.STRING=9]="STRING",n[n.BYTES=12]="BYTES",n[n.UINT32=13]="UINT32",n[n.SFIXED32=15]="SFIXED32",n[n.SFIXED64=16]="SFIXED64",n[n.SINT32=17]="SINT32",n[n.SINT64=18]="SINT64"})(oe||(oe={}));function Um(){let n=this.buf,e=this.pos,t=0,i=0;for(let r=0;r<28;r+=7){let o=n[e++];if(t|=(o&127)<<r,(o&128)==0){this.pos=e,this.assertBounds(),this.varint64Lo=t,this.varint64Hi=i;return}}let s=n[e++];if(t|=(s&15)<<28,i=(s&112)>>4,(s&128)==0){this.pos=e,this.assertBounds(),this.varint64Lo=t,this.varint64Hi=i;return}for(let r=3;r<=31;r+=7){let o=n[e++];if(i|=(o&127)<<r,(o&128)==0){this.pos=e,this.assertBounds(),this.varint64Lo=t,this.varint64Hi=i;return}}throw new Error("invalid varint")}var bc=4294967296;function ed(n){let e=n[0]==="-";e&&(n=n.slice(1));let t=1e6,i=0,s=0;function r(o,a){let l=Number(n.slice(o,a));s*=t,i=i*t+l,i>=bc&&(s=s+(i/bc|0),i=i%bc)}return r(-24,-18),r(-18,-12),r(-12,-6),r(-6),e?Pm(i,s):nd(i,s)}function Dm(n,e){let t=nd(n,e),i=t.hi&2147483648;i&&(t=Pm(t.lo,t.hi));let s=td(t.lo,t.hi);return i?"-"+s:s}function td(n,e){if({lo:n,hi:e}=rT(n,e),e<=2097151)return String(bc*e+n);let t=n&16777215,i=(n>>>24|e<<8)&16777215,s=e>>16&65535,r=t+i*6777216+s*6710656,o=i+s*8147497,a=s*2,l=1e7;return r>=l&&(o+=Math.floor(r/l),r%=l),o>=l&&(a+=Math.floor(o/l),o%=l),a.toString()+Lm(o)+Lm(r)}function rT(n,e){return{lo:n>>>0,hi:e>>>0}}function nd(n,e){return{lo:n|0,hi:e|0}}function Pm(n,e){return e=~e,n?n=~n+1:e+=1,nd(n,e)}var Lm=n=>{let e=String(n);return"0000000".slice(e.length)+e};function Gm(n,e){if(n>>>0<128){e.push(n);return}if(n>=0){for(;n>127;)e.push(n&127|128),n=n>>>7;e.push(n)}else{for(let t=0;t<9;t++)e.push(n&127|128),n=n>>7;e.push(1)}}function Vm(){let n=this.buf[this.pos++];if((n&128)===0)return this.assertBounds(),n;let e=n&127;if(n=this.buf[this.pos++],e|=(n&127)<<7,(n&128)===0)return this.assertBounds(),e;if(n=this.buf[this.pos++],e|=(n&127)<<14,(n&128)===0)return this.assertBounds(),e;if(n=this.buf[this.pos++],e|=(n&127)<<21,(n&128)===0)return this.assertBounds(),e;n=this.buf[this.pos++],e|=(n&15)<<28;for(let t=5;(n&128)!==0&&t<10;t++)n=this.buf[this.pos++];if((n&128)!==0)throw new Error("invalid varint");return this.assertBounds(),e>>>0}var Pt=oT();function oT(){let n=new DataView(new ArrayBuffer(8));if(typeof BigInt=="function"&&typeof n.getBigInt64=="function"&&typeof n.getBigUint64=="function"&&typeof n.setBigInt64=="function"&&typeof n.setBigUint64=="function"&&(!!globalThis.Deno||!!globalThis.Bun||typeof process!="object"||typeof process.env!="object"||process.env.BUF_BIGINT_DISABLE!=="1")){let t=BigInt("-9223372036854775808"),i=BigInt("9223372036854775807"),s=BigInt("0"),r=BigInt("18446744073709551615");return{zero:BigInt(0),supported:!0,parse(o){let a=typeof o=="bigint"?o:BigInt(o);if(a>i||a<t)throw new Error(`invalid int64: ${o}`);return a},uParse(o){let a=typeof o=="bigint"?o:BigInt(o);if(a>r||a<s)throw new Error(`invalid uint64: ${o}`);return a},enc(o){return n.setBigInt64(0,this.parse(o),!0),{lo:n.getInt32(0,!0),hi:n.getInt32(4,!0)}},uEnc(o){return n.setBigInt64(0,this.uParse(o),!0),{lo:n.getInt32(0,!0),hi:n.getInt32(4,!0)}},dec(o,a){return n.setInt32(0,o,!0),n.setInt32(4,a,!0),n.getBigInt64(0,!0)},uDec(o,a){return n.setInt32(0,o,!0),n.setInt32(4,a,!0),n.getBigUint64(0,!0)}}}return{zero:"0",supported:!1,parse(t){return typeof t!="string"&&(t=t.toString()),Om(t),t},uParse(t){return typeof t!="string"&&(t=t.toString()),km(t),t},enc(t){return typeof t!="string"&&(t=t.toString()),Om(t),ed(t)},uEnc(t){return typeof t!="string"&&(t=t.toString()),km(t),ed(t)},dec(t,i){return Dm(t,i)},uDec(t,i){return td(t,i)}}}function Om(n){if(!/^-?[0-9]+$/.test(n))throw new Error("invalid int64: "+n)}function km(n){if(!/^[0-9]+$/.test(n))throw new Error("invalid uint64: "+n)}function _a(n,e){switch(n){case oe.STRING:return"";case oe.BOOL:return!1;case oe.DOUBLE:case oe.FLOAT:return 0;case oe.INT64:case oe.UINT64:case oe.SFIXED64:case oe.FIXED64:case oe.SINT64:return e?"0":Pt.zero;case oe.BYTES:return new Uint8Array(0);default:return 0}}function Is(n,e){return Object.prototype.hasOwnProperty.call(n,e)&&n[e]!==void 0}function ba(n){return n!==null&&typeof n=="object"&&!Array.isArray(n)}function Sc(n){let e=n.fields[0];return lT(n.typeName)&&e!==void 0&&e.fieldKind=="scalar"&&e.name=="value"&&e.number==1}var aT=new Set(["google.protobuf.DoubleValue","google.protobuf.FloatValue","google.protobuf.Int64Value","google.protobuf.UInt64Value","google.protobuf.Int32Value","google.protobuf.UInt32Value","google.protobuf.BoolValue","google.protobuf.StringValue","google.protobuf.BytesValue"]);function lT(n){return aT.has(n)}var cT=999,uT=998,Wm=2;function Bs(n,e){return jh(e,n)?e:Hm(n)(e)}var zm=new WeakMap;function Hm(n){let e=zm.get(n);return e===void 0&&(e=hT(n),zm.set(n,e)),e}var id=0,Ym=1,Zm=2,Xm=3;function hT(n){let e=n.typeName,{properties:t,prototype:i}=dT(n);return s=>{let r;i!==void 0?(r=Object.create(i),r.$typeName=e):r={$typeName:e};for(let o=0;o<t.length;o++){let a=t[o],l=a.name,c=s?.[l];switch(a.kind){case id:c!=null?r[l]=a.convert!==void 0?a.convert(c):c:a.constant!==void 0&&(r[l]=a.constant);break;case Ym:r[l]=a.convert!==void 0&&Array.isArray(c)?c.map(a.convert):c??[];break;case Zm:if(a.convert===void 0||!ba(c))r[l]=c??{};else{let u={},d=Object.keys(c);for(let h=0;h<d.length;h++)u[d[h]]=a.convert(c[d[h]]);r[l]=u}break;case Xm:{let u=c;if(u?.case!=null){let d=a.convert.get(u.case);if(d!==void 0){r[l]={case:u.case,value:d(u.value)};break}}r[l]={case:void 0};break}}}return r}}function dT(n){var e,t;let i=[],s={},r=pT(n);for(let o of n.members){let a=o.localName;if(o.kind=="oneof"){i.push({name:a,kind:Xm,constant:void 0,convert:fT(o)});continue}switch(o.fieldKind){case"message":{i.push({name:a,kind:id,constant:void 0,convert:vc(o)});break}case"list":{i.push({name:a,kind:Ym,constant:void 0,convert:o.listKind=="message"?(e=vc(o))!==null&&e!==void 0?e:(l=>l):o.scalar==oe.BYTES?Sa:void 0});break}case"map":{i.push({name:a,kind:Zm,constant:void 0,convert:o.mapKind=="message"?(t=vc(o))!==null&&t!==void 0?t:(l=>l):o.scalar==oe.BYTES?Sa:void 0});break}default:{let l=mT(o);i.push({name:a,kind:id,constant:o.presence==Wm?l:void 0,convert:o.fieldKind=="scalar"&&o.scalar==oe.BYTES?Sa:void 0}),r&&(s[a]=l);break}}}return{properties:i,prototype:r?s:void 0}}function fT(n){let e=new Map;for(let t of n.fields){let i;t.fieldKind=="message"?i=vc(t):t.fieldKind=="scalar"&&t.scalar==oe.BYTES&&(i=Sa),e.set(t.localName,i??(s=>s))}return e}function vc(n){if(n.fieldKind=="message"&&!n.oneof&&Sc(n.message))return n.message.fields[0].scalar==oe.BYTES?Sa:void 0;if(n.message.typeName=="google.protobuf.Struct"&&n.parent.typeName!=="google.protobuf.Value")return;let e=n.message,t;return i=>!ba(i)||jh(i,e)?i:(t??(t=Hm(e)),t(i))}function Sa(n){return Array.isArray(n)?new Uint8Array(n):n}function pT(n){switch(n.file.edition){case cT:return!1;case uT:return!0;default:return n.fields.some(e=>e.presence!=Wm&&e.fieldKind!="message"&&!e.oneof)}}function mT(n){let e=n.getDefaultValue();return e!==void 0?n.fieldKind=="scalar"&&n.longAsString?e.toString():e:n.fieldKind=="scalar"?_a(n.scalar,n.longAsString):n.enum.values[0].number}var Ac=class extends Error{constructor(e,t,i="FieldValueInvalidError"){super(t),this.name=i,this.field=()=>e}};var sd;function gT(n){var e;sd=Object.assign(Object.assign({},n),{encodeUtf8Into:(e=n.encodeUtf8Into)!==null&&e!==void 0?e:od(n.encodeUtf8.bind(n))})}function rd(){if(!sd){let n=globalThis;if(!n.TextEncoder||!n.TextDecoder)throw new Error("encoding API missing: install TextEncoder and TextDecoder on globalThis");let e=new n.TextEncoder,t=new n.TextDecoder,i,s={encodeUtf8(o){return e.encode(o)},decodeUtf8(o,a){return a?(i||(i=new n.TextDecoder("utf-8",{fatal:!0})),i.decode(o)):t.decode(o)},checkUtf8(o){try{return encodeURIComponent(o),!0}catch{return!1}}};e.encodeInto&&(s.encodeUtf8Into=e.encodeInto.bind(e));let r=String.prototype.isWellFormed;r&&(s.checkUtf8=o=>r.call(o)),gT(s)}return sd}function od(n){return(e,t)=>{let i=n(e);return t.set(i),{written:i.byteLength}}}var Et;(function(n){n[n.Varint=0]="Varint",n[n.Bit64=1]="Bit64",n[n.LengthDelimited=2]="LengthDelimited",n[n.StartGroup=3]="StartGroup",n[n.EndGroup=4]="EndGroup",n[n.Bit32=5]="Bit32"})(Et||(Et={}));var xT=34028234663852886e22,yT=-34028234663852886e22,_T=4294967295,bT=2147483647,ST=-2147483648,Tc=class{constructor(e){this.stackPos=[],this.encodeUtf8Into=e?od(e):rd().encodeUtf8Into,this.buffer=$m,this.viewCache=AT,this.pos=0}ensureCapacity(e){let t=this.pos+e;if(t>this.buffer.length){let i=this.buffer.length||vT;for(;i<t;)i*=2;let s=new Uint8Array(i);this.pos>0&&s.set(this.buffer),this.buffer=s}}view(){let e=this.buffer,t=this.viewCache;if(t.byteLength===e.byteLength)return t;let i=new DataView(e.buffer);return this.viewCache=i,i}finish(){let e=this.buffer.slice(0,this.pos);return this.pos=0,this.stackPos=[],e}fork(){return this.stackPos.push(this.pos),this.ensureCapacity(va),this.buffer[this.pos++]=0,this}join(){let e=this.stackPos.pop();if(e===void 0)throw new Error("invalid state, fork stack empty");let t=this.pos-e-va,i=ad(t);return i>va&&(this.ensureCapacity(i-va),this.buffer.copyWithin(e+i,e+va,this.pos)),this.pos=e,this.uint32(t),this.pos+=t,this}tag(e,t){return this.uint32((e<<3|t)>>>0)}raw(e){return this.ensureCapacity(e.length),this.buffer.set(e,this.pos),this.pos+=e.length,this}uint32(e){if(Km(e),this.ensureCapacity(5),e<128)return this.buffer[this.pos++]=e,this;for(;e>127;)this.buffer[this.pos++]=e&127|128,e>>>=7;return this.buffer[this.pos++]=e,this}int32(e){if(ld(e),e>=0)return this.uint32(e);this.ensureCapacity(10);for(let t=0;t<9;t++)this.buffer[this.pos++]=e&127|128,e>>=7;return this.buffer[this.pos++]=1,this}bool(e){return this.ensureCapacity(1),this.buffer[this.pos++]=e?1:0,this}bytes(e){return this.uint32(e.byteLength),this.raw(e)}string(e){typeof e!="string"&&(e=String(e));let t=e.length;if(t<=Jm){this.ensureCapacity(t+1);let l=this.buffer,c=this.pos;l[c++]=t;let u=0;for(;u<t;u++){let d=e.charCodeAt(u);if(d>127)break;l[c++]=d}if(u==t)return this.pos=c,this}this.ensureCapacity(t*3+5);let i=ad(t),s=this.buffer,r=this.pos,{written:o}=this.encodeUtf8Into(e,s.subarray(r+i)),a=ad(o);return a!=i&&s.copyWithin(r+a,r+i,r+i+o),this.uint32(o),this.pos+=o,this}float(e){return TT(e),this.ensureCapacity(4),this.view().setFloat32(this.pos,e,!0),this.pos+=4,this}double(e){return this.ensureCapacity(8),this.view().setFloat64(this.pos,e,!0),this.pos+=8,this}fixed32(e){return Km(e),this.ensureCapacity(4),this.view().setUint32(this.pos,e,!0),this.pos+=4,this}sfixed32(e){return ld(e),this.ensureCapacity(4),this.view().setInt32(this.pos,e,!0),this.pos+=4,this}sint32(e){return ld(e),this.uint32((e<<1^e>>31)>>>0)}sfixed64(e){let t=Pt.enc(e);this.ensureCapacity(8);let i=this.view();return i.setInt32(this.pos,t.lo,!0),i.setInt32(this.pos+4,t.hi,!0),this.pos+=8,this}fixed64(e){let t=Pt.uEnc(e);this.ensureCapacity(8);let i=this.view();return i.setInt32(this.pos,t.lo,!0),i.setInt32(this.pos+4,t.hi,!0),this.pos+=8,this}int64(e){let t=Pt.enc(e);return this.writeVarint64(t.lo,t.hi)}sint64(e){let t=Pt.enc(e),i=t.hi>>31,s=t.lo<<1^i,r=(t.hi<<1|t.lo>>>31)^i;return this.writeVarint64(s,r)}uint64(e){let t=Pt.uEnc(e);return this.writeVarint64(t.lo,t.hi)}writeVarint64(e,t){this.ensureCapacity(10);let i=this.buffer,s=this.pos;for(let a=0;a<28;a=a+7){let l=e>>>a,c=!(!(l>>>7)&&t==0);if(i[s++]=(c?l|128:l)&255,!c)return this.pos=s,this}let r=e>>>28&15|(t&7)<<4,o=t>>3!=0;if(i[s++]=(o?r|128:r)&255,!o)return this.pos=s,this;for(let a=3;a<31;a=a+7){let l=t>>>a,c=!!(l>>>7);if(i[s++]=(c?l|128:l)&255,!c)return this.pos=s,this}return i[s++]=t>>>31&1,this.pos=s,this}},vT=128,va=1,$m=new Uint8Array(0),AT=new DataView($m.buffer),Jm=32;function ad(n){return n<128?1:n<16384?2:n<2097152?3:n<268435456?4:5}var Ec=class{constructor(e,t=rd().decodeUtf8){this.decodeUtf8=t,this.varint64Lo=0,this.varint64Hi=0,this.varint64=Um,this.uint32=Vm,this.buf=e,this.len=e.length,this.pos=0,this.view=new DataView(e.buffer,e.byteOffset,e.byteLength)}tag(){let e=this.pos,t=this.uint32(),i=this.pos-e;if(i>5||i==5&&this.buf[this.pos-1]>15)throw new Error("illegal tag: varint overflows uint32");let s=t>>>3,r=t&7;if(s<=0||r>5)throw new Error("illegal tag: field no "+s+" wire type "+r);return[s,r]}skip(e,t,i=100){let s=this.pos;switch(e){case Et.Varint:for(;this.buf[this.pos++]&128;);break;case Et.Bit64:this.pos+=4;case Et.Bit32:this.pos+=4;break;case Et.LengthDelimited:let r=this.uint32();this.pos+=r;break;case Et.StartGroup:if(i<=0)throw new Error("maximum recursion depth reached");for(;;){let[o,a]=this.tag();if(a===Et.EndGroup){if(t!==void 0&&o!==t)throw new Error("invalid end group tag");break}this.skip(a,o,i-1)}break;default:throw new Error("cant skip wire type "+e)}return this.assertBounds(),this.buf.subarray(s,this.pos)}assertBounds(){if(this.pos>this.len)throw new RangeError("premature EOF")}int32(){return this.uint32()|0}sint32(){let e=this.uint32();return e>>>1^-(e&1)}int64(){return this.varint64(),Pt.dec(this.varint64Lo,this.varint64Hi)}uint64(){return this.varint64(),Pt.uDec(this.varint64Lo,this.varint64Hi)}sint64(){this.varint64();let e=this.varint64Lo,t=this.varint64Hi,i=-(e&1);return e=(e>>>1|(t&1)<<31)^i,t=t>>>1^i,Pt.dec(e,t)}bool(){let e=this.buf[this.pos];return e<128?(this.pos++,e!==0):(this.varint64(),this.varint64Lo!==0||this.varint64Hi!==0)}fixed32(){return this.view.getUint32((this.pos+=4)-4,!0)}sfixed32(){return this.view.getInt32((this.pos+=4)-4,!0)}fixed64(){return Pt.uDec(this.sfixed32(),this.sfixed32())}sfixed64(){return Pt.dec(this.sfixed32(),this.sfixed32())}float(){return this.view.getFloat32((this.pos+=4)-4,!0)}double(){return this.view.getFloat64((this.pos+=8)-8,!0)}bytes(){let e=this.uint32(),t=this.pos;return this.pos+=e,this.assertBounds(),this.buf.subarray(t,t+e)}string(e){let t=this.bytes(),i=t.length;if(i<=Jm){let s=new Array(i);for(let r=0;r<i;r++){let o=t[r];if(o>127)return this.decodeUtf8(t,e);s[r]=o}return String.fromCharCode.apply(String,s)}return this.decodeUtf8(t,e)}};function ld(n){if(typeof n=="string")n=Number(n);else if(typeof n!="number")throw new Error("invalid int32: "+typeof n);if(!Number.isInteger(n)||n>bT||n<ST)throw new Error("invalid int32: "+n)}function Km(n){if(typeof n=="string")n=Number(n);else if(typeof n!="number")throw new Error("invalid uint32: "+typeof n);if(!Number.isInteger(n)||n>_T||n<0)throw new Error("invalid uint32: "+n)}function TT(n){if(typeof n=="string"){let e=n;if(n=Number(n),Number.isNaN(n)&&e!=="NaN")throw new Error("invalid float32: "+e)}else if(typeof n!="number")throw new Error("invalid float32: "+typeof n);if(Number.isFinite(n)&&(n>xT||n<yT))throw new Error("invalid float32: "+n)}var ET=0;function Ji(n){if(MT(n))return{toMessage:t=>qm(t),toLocal:t=>Qm(t)};if(n.fieldKind=="message"&&!n.oneof&&Sc(n.message)){let t=n.message,i=t.fields[0].localName;return{toMessage:s=>{let r=Bs(t);return s!==void 0&&(r[i]=s),r},toLocal:s=>s[i]}}let e=n.message;return{toMessage:t=>t===void 0?Bs(e):t,toLocal:t=>t}}function MT(n){return n.message.typeName=="google.protobuf.Struct"&&n.parent.typeName!="google.protobuf.Value"}function qm(n){let e={$typeName:"google.protobuf.Struct",fields:{}};if(ba(n))for(let t of Object.keys(n))e.fields[t]=eg(n[t]);return e}function Qm(n){let e={};for(let t of Object.keys(n.fields))e[t]=jm(n.fields[t]);return e}function jm(n){switch(n.kind.case){case"structValue":return Qm(n.kind.value);case"listValue":return n.kind.value.values.map(jm);case"nullValue":case void 0:return null;default:return n.kind.value}}function eg(n){let e={$typeName:"google.protobuf.Value",kind:{case:void 0}};switch(typeof n){case"number":e.kind={case:"numberValue",value:n};break;case"string":e.kind={case:"stringValue",value:n};break;case"boolean":e.kind={case:"boolValue",value:n};break;case"object":if(n===null)e.kind={case:"nullValue",value:ET};else if(Array.isArray(n)){let t={$typeName:"google.protobuf.ListValue",values:[]};if(Array.isArray(n))for(let i of n)t.values.push(eg(i));e.kind={case:"listValue",value:t}}else e.kind={case:"structValue",value:qm(n)};break}return e}var tg=Uint8Array.prototype.setFromBase64;function ig(n){let e=n.length,t=e-(e+3>>2);(e&3)==0&&n[e-1]=="="&&(t-=n[e-2]=="="?2:1);let i=new Uint8Array(t),s=-1;if(tg)try{let r=tg.call(i,n);r.read==e&&(s=r.written)}catch{}return s<0&&(s=RT(i,n)),s==t?i:i.subarray(0,s)}function RT(n,e){let t=CT(),i=0,s=0,r,o=0;for(let a=0;a<e.length;a++){if(r=t[e.charCodeAt(a)],r===void 0)switch(e[a]){case"=":s=0;case`
`:case"\r":case"	":case" ":continue;default:throw Error("invalid base64 string")}switch(s){case 0:o=r,s=1;break;case 1:n[i++]=o<<2|(r&48)>>4,o=r,s=2;break;case 2:n[i++]=(o&15)<<4|(r&60)>>2,o=r,s=3;break;case 3:n[i++]=(o&3)<<6|r,s=0;break}}if(s==1)throw Error("invalid base64 string");return i}var rB=Uint8Array.prototype.toBase64;var Mc,ng,Vr;function wT(n){return Mc||(Mc="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/".split(""),ng=Mc.slice(0,-2).concat("-","_")),n=="url"?ng:Mc}function CT(){if(!Vr){Vr=[];let n=wT("std");for(let e=0;e<n.length;e++)Vr[n[e].charCodeAt(0)]=e;Vr[45]=n.indexOf("+"),Vr[95]=n.indexOf("/")}return Vr}function Or(n){let e=!1,t=[];for(let i=0;i<n.length;i++){let s=n.charAt(i);switch(s){case"_":e=!0;break;case"0":case"1":case"2":case"3":case"4":case"5":case"6":case"7":case"8":case"9":t.push(s),e=!1;break;default:e&&(e=!1,s=s.toUpperCase()),t.push(s);break}}return t.join("")}var NT=new Set(["constructor","toString","toJSON","valueOf"]);function kr(n){return NT.has(n)?n+"$":n}function Aa(n){for(let e of n.field)Is(e,"jsonName")||(e.jsonName=Or(e.name));n.nestedType.forEach(Aa)}function sg(n,e){let t=n.values.find(i=>i.name===e);if(!t)throw new Error(`cannot parse ${n} default value: ${e}`);return t.number}function rg(n,e){switch(n){case oe.STRING:return e;case oe.BYTES:{let t=IT(e);if(t===!1)throw new Error(`cannot parse ${oe[n]} default value: ${e}`);return t}case oe.INT64:case oe.SFIXED64:case oe.SINT64:return Pt.parse(e);case oe.UINT64:case oe.FIXED64:return Pt.uParse(e);case oe.DOUBLE:case oe.FLOAT:switch(e){case"inf":return Number.POSITIVE_INFINITY;case"-inf":return Number.NEGATIVE_INFINITY;case"nan":return Number.NaN;default:return parseFloat(e)}case oe.BOOL:return e==="true";case oe.INT32:case oe.UINT32:case oe.SINT32:case oe.FIXED32:case oe.SFIXED32:return parseInt(e,10)}}function IT(n){let e=[],t={tail:n,c:"",next(){return this.tail.length==0?!1:(this.c=this.tail[0],this.tail=this.tail.substring(1),!0)},take(i){if(this.tail.length>=i){let s=this.tail.substring(0,i);return this.tail=this.tail.substring(i),s}return!1}};for(;t.next();)switch(t.c){case"\\":if(t.next())switch(t.c){case"\\":e.push(t.c.charCodeAt(0));break;case"b":e.push(8);break;case"f":e.push(12);break;case"n":e.push(10);break;case"r":e.push(13);break;case"t":e.push(9);break;case"v":e.push(11);break;case"0":case"1":case"2":case"3":case"4":case"5":case"6":case"7":{let i=t.c,s=t.take(2);if(s===!1)return!1;let r=parseInt(i+s,8);if(Number.isNaN(r))return!1;e.push(r);break}case"x":{let i=t.c,s=t.take(2);if(s===!1)return!1;let r=parseInt(i+s,16);if(Number.isNaN(r))return!1;e.push(r);break}case"u":{let i=t.c,s=t.take(4);if(s===!1)return!1;let r=parseInt(i+s,16);if(Number.isNaN(r))return!1;let o=new Uint8Array(4);new DataView(o.buffer).setInt32(0,r,!0),e.push(o[0],o[1],o[2],o[3]);break}case"U":{let i=t.c,s=t.take(8);if(s===!1)return!1;let r=Pt.uEnc(i+s),o=new Uint8Array(8),a=new DataView(o.buffer);a.setInt32(0,r.lo,!0),a.setInt32(4,r.hi,!0),e.push(o[0],o[1],o[2],o[3],o[4],o[5],o[6],o[7]);break}}break;default:e.push(t.c.charCodeAt(0))}return new Uint8Array(e)}function*Rc(n){switch(n.kind){case"file":for(let e of n.messages)yield e,yield*Rc(e);yield*n.enums,yield*n.services,yield*n.extensions;break;case"message":for(let e of n.nestedMessages)yield e,yield*Rc(e);yield*n.nestedEnums,yield*n.nestedExtensions;break}}function wc(...n){let e=BT();if(!n.length)return e;if("$typeName"in n[0]&&n[0].$typeName=="google.protobuf.FileDescriptorSet"){for(let t of n[0].file)cg(t,e);return e}if("$typeName"in n[0]){let r=function(o){let a=[];for(let l of o.dependency){if(e.getFile(l)!=null||s.has(l))continue;let c=i(l);if(!c)throw new Error(`Unable to resolve ${l}, imported by ${o.name}`);"kind"in c?e.addFile(c,!1,!0):(s.add(c.name),a.push(c))}return a.concat(...a.map(r))},t=n[0],i=n[1],s=new Set;for(let o of[t,...r(t)].reverse())cg(o,e)}else for(let t of n)for(let i of t.files)e.addFile(i);return e}function BT(){let n=new Map,e=new Map,t=new Map;return{kind:"registry",types:n,extendees:e,[Symbol.iterator](){return n.values()},get files(){return t.values()},addFile(i,s,r){if(t.set(i.proto.name,i),!s)for(let o of Rc(i))this.add(o);if(r)for(let o of i.dependencies)this.addFile(o,s,r)},add(i){if(i.kind=="extension"){let s=e.get(i.extendee.typeName);s||e.set(i.extendee.typeName,s=new Map),s.set(i.number,i)}n.set(i.typeName,i)},get(i){return n.get(i)},getFile(i){return t.get(i)},getMessage(i){let s=n.get(i);return s?.kind=="message"?s:void 0},getEnum(i){let s=n.get(i);return s?.kind=="enum"?s:void 0},getExtension(i){let s=n.get(i);return s?.kind=="extension"?s:void 0},getExtensionFor(i,s){var r;return(r=e.get(i.typeName))===null||r===void 0?void 0:r.get(s)},getService(i){let s=n.get(i);return s?.kind=="service"?s:void 0}}}var FT=998,LT=999,UT=9999,DT=9,Ea=10,Ta=11,PT=12,og=14,fd=3,GT=2,ag=1,VT=0,cd=1,lg=2,OT=3,kT=1,zT=2,WT=1,HT=2;var YT=1001,hg={998:{fieldPresence:1,enumType:2,repeatedFieldEncoding:2,utf8Validation:3,messageEncoding:1,jsonFormat:2,enforceNamingStyle:2,defaultSymbolVisibility:1},999:{fieldPresence:2,enumType:1,repeatedFieldEncoding:1,utf8Validation:2,messageEncoding:1,jsonFormat:1,enforceNamingStyle:2,defaultSymbolVisibility:1},1e3:{fieldPresence:1,enumType:1,repeatedFieldEncoding:1,utf8Validation:2,messageEncoding:1,jsonFormat:1,enforceNamingStyle:2,defaultSymbolVisibility:1},1001:{fieldPresence:1,enumType:1,repeatedFieldEncoding:1,utf8Validation:2,messageEncoding:1,jsonFormat:1,enforceNamingStyle:1,defaultSymbolVisibility:2}};function cg(n,e){var t,i;let s={kind:"file",proto:n,deprecated:(i=(t=n.options)===null||t===void 0?void 0:t.deprecated)!==null&&i!==void 0?i:!1,edition:$T(n),name:n.name.replace(/\.proto$/,""),dependencies:JT(n,e),enums:[],messages:[],extensions:[],services:[],toString(){return`file ${n.name}`}},r=new Map,o={get(a){return r.get(a)},add(a){var l;jn(((l=a.proto.options)===null||l===void 0?void 0:l.mapEntry)===!0),r.set(a.typeName,a)}};for(let a of n.enumType)dg(a,s,void 0,e);for(let a of n.messageType)fg(a,s,void 0,e,o);for(let a of n.service)ZT(a,s,e);ud(s,e);for(let a of r.values())hd(a,e,o);for(let a of s.messages)hd(a,e,o),ud(a,e);e.addFile(s,!0)}function ud(n,e){switch(n.kind){case"file":for(let t of n.proto.extension){let i=dd(t,n,e);n.extensions.push(i),e.add(i)}break;case"message":for(let t of n.proto.extension){let i=dd(t,n,e);n.nestedExtensions.push(i),e.add(i)}for(let t of n.nestedMessages)ud(t,e);break}}function hd(n,e,t){let i=n.proto.oneofDecl.map(r=>KT(r,n)),s=new Set;for(let r of n.proto.field){let o=jT(r,i),a=dd(r,n,e,o,t);n.fields.push(a),n.field[a.localName]=a,o===void 0?n.members.push(a):(o.fields.push(a),s.has(o)||(s.add(o),n.members.push(o)))}for(let r of i.filter(o=>s.has(o)))n.oneofs.push(r);for(let r of n.nestedMessages)hd(r,e,t)}function dg(n,e,t,i){var s,r,o,a,l;let c=qT(n.name,n.value),u={kind:"enum",proto:n,deprecated:(r=(s=n.options)===null||s===void 0?void 0:s.deprecated)!==null&&r!==void 0?r:!1,file:e,parent:t,open:!0,name:n.name,typeName:Cc(n,t,e),value:{},values:[],sharedPrefix:c,toString(){return`enum ${this.typeName}`}};u.open=iE(u),i.add(u);for(let d of n.value){let h=d.name;u.values.push(u.value[d.number]={kind:"enum_value",proto:d,deprecated:(a=(o=d.options)===null||o===void 0?void 0:o.deprecated)!==null&&a!==void 0?a:!1,parent:u,name:h,localName:kr(c==null?h:h.substring(c.length)),number:d.number,toString(){return`enum value ${u.typeName}.${h}`}})}((l=t?.nestedEnums)!==null&&l!==void 0?l:e.enums).push(u)}function fg(n,e,t,i,s){var r,o,a,l;let c={kind:"message",proto:n,deprecated:(o=(r=n.options)===null||r===void 0?void 0:r.deprecated)!==null&&o!==void 0?o:!1,file:e,parent:t,name:n.name,typeName:Cc(n,t,e),fields:[],field:{},oneofs:[],members:[],nestedEnums:[],nestedMessages:[],nestedExtensions:[],toString(){return`message ${this.typeName}`}};((a=n.options)===null||a===void 0?void 0:a.mapEntry)===!0?s.add(c):(((l=t?.nestedMessages)!==null&&l!==void 0?l:e.messages).push(c),i.add(c));for(let u of n.enumType)dg(u,e,c,i);for(let u of n.nestedType)fg(u,e,c,i,s)}function ZT(n,e,t){var i,s;let r={kind:"service",proto:n,deprecated:(s=(i=n.options)===null||i===void 0?void 0:i.deprecated)!==null&&s!==void 0?s:!1,file:e,name:n.name,typeName:Cc(n,void 0,e),methods:[],method:{},toString(){return`service ${this.typeName}`}};e.services.push(r),t.add(r);for(let o of n.method){let a=XT(o,r,t);r.methods.push(a),r.method[a.localName]=a}}function XT(n,e,t){var i,s,r,o;let a;n.clientStreaming&&n.serverStreaming?a="bidi_streaming":n.clientStreaming?a="client_streaming":n.serverStreaming?a="server_streaming":a="unary";let l=t.getMessage(Fi(n.inputType)),c=t.getMessage(Fi(n.outputType));jn(l,`invalid MethodDescriptorProto: input_type ${n.inputType} not found`),jn(c,`invalid MethodDescriptorProto: output_type ${n.inputType} not found`);let u=n.name;return{kind:"rpc",proto:n,deprecated:(s=(i=n.options)===null||i===void 0?void 0:i.deprecated)!==null&&s!==void 0?s:!1,parent:e,name:u,localName:kr(u.length?kr(u[0].toLowerCase()+u.substring(1)):u),methodKind:a,input:l,output:c,idempotency:(o=(r=n.options)===null||r===void 0?void 0:r.idempotencyLevel)!==null&&o!==void 0?o:VT,toString(){return`rpc ${e.typeName}.${u}`}}}function KT(n,e){return{kind:"oneof",proto:n,deprecated:!1,parent:e,fields:[],name:n.name,localName:kr(Or(n.name)),toString(){return`oneof ${e.typeName}.${this.name}`}}}function dd(n,e,t,i,s){var r,o,a;let l=s===void 0,c={kind:"field",proto:n,deprecated:(o=(r=n.options)===null||r===void 0?void 0:r.deprecated)!==null&&o!==void 0?o:!1,name:n.name,number:n.number,scalar:void 0,message:void 0,enum:void 0,presence:eE(n,i,l,e),utf8Validation:sE(n,e),listKind:void 0,mapKind:void 0,mapKey:void 0,delimitedEncoding:void 0,packed:void 0,longAsString:!1,getDefaultValue:void 0},u;if(l){let g=e.kind=="file"?e:e.file,y=e.kind=="file"?void 0:e,m=Cc(n,y,g);c.kind="extension",c.file=g,c.parent=y,c.oneof=void 0,c.typeName=m,c.jsonName=`[${m}]`,u=()=>`extension ${m}`;let p=t.getMessage(Fi(n.extendee));jn(p,`invalid FieldDescriptorProto: extendee ${n.extendee} not found`),c.extendee=p}else{let g=e;jn(g.kind=="message"),c.parent=g,c.oneof=i,c.localName=i?Or(n.name):kr(Or(n.name)),c.jsonName=n.jsonName,u=()=>`field ${g.typeName}.${n.name}`}Object.defineProperty(c,"toString",{value:u,writable:!0,enumerable:!0,configurable:!0});let d=n.label,h=n.type,f=(a=n.options)===null||a===void 0?void 0:a.jstype;if(d===fd){let g=h==Ta?s?.get(Fi(n.typeName)):void 0;if(g){c.fieldKind="map";let{key:y,value:m}=nE(g);return c.mapKey=y.scalar,c.mapKind=m.fieldKind,c.message=m.message,c.delimitedEncoding=!1,c.enum=m.enum,c.scalar=m.scalar,c}switch(c.fieldKind="list",h){case Ta:case Ea:c.listKind="message",c.message=t.getMessage(Fi(n.typeName)),jn(c.message),c.delimitedEncoding=ug(n,e);break;case og:c.listKind="enum",c.enum=t.getEnum(Fi(n.typeName)),jn(c.enum);break;default:c.listKind="scalar",c.scalar=h,c.longAsString=f==ag;break}return c.packed=tE(n,e),c}switch(h){case Ta:case Ea:c.fieldKind="message",c.message=t.getMessage(Fi(n.typeName)),jn(c.message,`invalid FieldDescriptorProto: type_name ${n.typeName} not found`),c.delimitedEncoding=ug(n,e),c.getDefaultValue=()=>{};break;case og:{let g=t.getEnum(Fi(n.typeName));jn(g!==void 0,`invalid FieldDescriptorProto: type_name ${n.typeName} not found`),c.fieldKind="enum",c.enum=t.getEnum(Fi(n.typeName)),c.getDefaultValue=()=>Is(n,"defaultValue")?sg(g,n.defaultValue):void 0;break}default:{c.fieldKind="scalar",c.scalar=h,c.longAsString=f==ag,c.getDefaultValue=()=>Is(n,"defaultValue")?rg(h,n.defaultValue):void 0;break}}return c}function $T(n){switch(n.syntax){case"":case"proto2":return FT;case"proto3":return LT;case"editions":if(n.edition===UT)return YT;if(n.edition in hg)return n.edition;throw new Error(`${n.name}: unsupported edition`);default:throw new Error(`${n.name}: unsupported syntax "${n.syntax}"`)}}function JT(n,e){return n.dependency.map(t=>{let i=e.getFile(t);if(!i)throw new Error(`Cannot find ${t}, imported by ${n.name}`);return i})}function qT(n,e){let t=QT(n)+"_";for(let i of e){if(!i.name.toLowerCase().startsWith(t))return;let s=i.name.substring(t.length);if(s.length==0||/^\d/.test(s))return}return t}function QT(n){return(n.substring(0,1)+n.substring(1).replace(/[A-Z]/g,e=>"_"+e)).toLowerCase()}function Cc(n,e,t){let i;return e?i=`${e.typeName}.${n.name}`:t.proto.package.length>0?i=`${t.proto.package}.${n.name}`:i=`${n.name}`,i}function Fi(n){return n.startsWith(".")?n.substring(1):n}function jT(n,e){if(!Is(n,"oneofIndex")||n.proto3Optional)return;let t=e[n.oneofIndex];return jn(t,`invalid FieldDescriptorProto: oneof #${n.oneofIndex} for field #${n.number} not found`),t}function eE(n,e,t,i){if(n.label==GT)return OT;if(n.label==fd)return lg;if(e||n.proto3Optional||t)return cd;let s=Fs("fieldPresence",{proto:n,parent:i});return s==lg&&(n.type==Ta||n.type==Ea)?cd:s}function tE(n,e){if(n.label!=fd)return!1;switch(n.type){case DT:case PT:case Ea:case Ta:return!1}let t=n.options;return t&&Is(t,"packed")?t.packed:kT==Fs("repeatedFieldEncoding",{proto:n,parent:e})}function nE(n){let e=n.fields.find(i=>i.number===1),t=n.fields.find(i=>i.number===2);return jn(e&&e.fieldKind=="scalar"&&e.scalar!=oe.BYTES&&e.scalar!=oe.FLOAT&&e.scalar!=oe.DOUBLE&&t&&t.fieldKind!="list"&&t.fieldKind!="map"),{key:e,value:t}}function iE(n){var e;return WT==Fs("enumType",{proto:n.proto,parent:(e=n.parent)!==null&&e!==void 0?e:n.file})}function ug(n,e){return n.type==Ea?!0:zT==Fs("messageEncoding",{proto:n,parent:e})}function sE(n,e){return HT==Fs("utf8Validation",{proto:n,parent:e})}function Fs(n,e){var t,i;let s=(t=e.proto.options)===null||t===void 0?void 0:t.features;if(s){let r=s[n];if(r!=0)return r}if("kind"in e){if(e.kind=="message")return Fs(n,(i=e.parent)!==null&&i!==void 0?i:e.file);let r=hg[e.edition];if(!r)throw new Error(`feature default for edition ${e.edition} not found`);return r[n]}return Fs(n,e.parent)}function jn(n,e){if(!n)throw new Error(e)}function pg(n){let e=rE(n);return e.messageType.forEach(Aa),wc(e,()=>{}).getFile(e.name)}function rE(n){return Object.assign(Object.create({syntax:"",edition:0}),Object.assign(Object.assign({$typeName:"google.protobuf.FileDescriptorProto",dependency:[],publicDependency:[],weakDependency:[],optionDependency:[],service:[],extension:[]},n),{messageType:n.messageType.map(mg),enumType:n.enumType.map(gg)}))}function mg(n){var e,t,i,s,r,o,a,l;return Object.assign(Object.create({visibility:0}),{$typeName:"google.protobuf.DescriptorProto",name:n.name,field:(t=(e=n.field)===null||e===void 0?void 0:e.map(oE))!==null&&t!==void 0?t:[],extension:[],nestedType:(s=(i=n.nestedType)===null||i===void 0?void 0:i.map(mg))!==null&&s!==void 0?s:[],enumType:(o=(r=n.enumType)===null||r===void 0?void 0:r.map(gg))!==null&&o!==void 0?o:[],extensionRange:(l=(a=n.extensionRange)===null||a===void 0?void 0:a.map(u=>Object.assign({$typeName:"google.protobuf.DescriptorProto.ExtensionRange"},u)))!==null&&l!==void 0?l:[],oneofDecl:[],reservedRange:[],reservedName:[]})}function oE(n){return Object.assign(Object.create({label:1,typeName:"",extendee:"",defaultValue:"",oneofIndex:0,jsonName:"",proto3Optional:!1}),Object.assign(Object.assign({$typeName:"google.protobuf.FieldDescriptorProto"},n),{options:n.options?aE(n.options):void 0}))}function aE(n){var e,t,i;return Object.assign(Object.create({ctype:0,packed:!1,jstype:0,lazy:!1,unverifiedLazy:!1,deprecated:!1,weak:!1,debugRedact:!1,retention:0}),Object.assign(Object.assign({$typeName:"google.protobuf.FieldOptions"},n),{targets:(e=n.targets)!==null&&e!==void 0?e:[],editionDefaults:(i=(t=n.editionDefaults)===null||t===void 0?void 0:t.map(r=>Object.assign({$typeName:"google.protobuf.FieldOptions.EditionDefault"},r)))!==null&&i!==void 0?i:[],uninterpretedOption:[]}))}function gg(n){return Object.assign(Object.create({visibility:0}),{$typeName:"google.protobuf.EnumDescriptorProto",name:n.name,reservedName:[],reservedRange:[],value:n.value.map(t=>Object.assign({$typeName:"google.protobuf.EnumValueDescriptorProto"},t))})}function qi(n,e,...t){return t.reduce((i,s)=>i.nestedMessages[s],n.messages[e])}var pd=pg({name:"google/protobuf/descriptor.proto",package:"google.protobuf",messageType:[{name:"FileDescriptorSet",field:[{name:"file",number:1,type:11,label:3,typeName:".google.protobuf.FileDescriptorProto"}],extensionRange:[{start:536e6,end:536000001}]},{name:"FileDescriptorProto",field:[{name:"name",number:1,type:9,label:1},{name:"package",number:2,type:9,label:1},{name:"dependency",number:3,type:9,label:3},{name:"public_dependency",number:10,type:5,label:3},{name:"weak_dependency",number:11,type:5,label:3},{name:"option_dependency",number:15,type:9,label:3},{name:"message_type",number:4,type:11,label:3,typeName:".google.protobuf.DescriptorProto"},{name:"enum_type",number:5,type:11,label:3,typeName:".google.protobuf.EnumDescriptorProto"},{name:"service",number:6,type:11,label:3,typeName:".google.protobuf.ServiceDescriptorProto"},{name:"extension",number:7,type:11,label:3,typeName:".google.protobuf.FieldDescriptorProto"},{name:"options",number:8,type:11,label:1,typeName:".google.protobuf.FileOptions"},{name:"source_code_info",number:9,type:11,label:1,typeName:".google.protobuf.SourceCodeInfo"},{name:"syntax",number:12,type:9,label:1},{name:"edition",number:14,type:14,label:1,typeName:".google.protobuf.Edition"}]},{name:"DescriptorProto",field:[{name:"name",number:1,type:9,label:1},{name:"field",number:2,type:11,label:3,typeName:".google.protobuf.FieldDescriptorProto"},{name:"extension",number:6,type:11,label:3,typeName:".google.protobuf.FieldDescriptorProto"},{name:"nested_type",number:3,type:11,label:3,typeName:".google.protobuf.DescriptorProto"},{name:"enum_type",number:4,type:11,label:3,typeName:".google.protobuf.EnumDescriptorProto"},{name:"extension_range",number:5,type:11,label:3,typeName:".google.protobuf.DescriptorProto.ExtensionRange"},{name:"oneof_decl",number:8,type:11,label:3,typeName:".google.protobuf.OneofDescriptorProto"},{name:"options",number:7,type:11,label:1,typeName:".google.protobuf.MessageOptions"},{name:"reserved_range",number:9,type:11,label:3,typeName:".google.protobuf.DescriptorProto.ReservedRange"},{name:"reserved_name",number:10,type:9,label:3},{name:"visibility",number:11,type:14,label:1,typeName:".google.protobuf.SymbolVisibility"}],nestedType:[{name:"ExtensionRange",field:[{name:"start",number:1,type:5,label:1},{name:"end",number:2,type:5,label:1},{name:"options",number:3,type:11,label:1,typeName:".google.protobuf.ExtensionRangeOptions"}]},{name:"ReservedRange",field:[{name:"start",number:1,type:5,label:1},{name:"end",number:2,type:5,label:1}]}]},{name:"ExtensionRangeOptions",field:[{name:"uninterpreted_option",number:999,type:11,label:3,typeName:".google.protobuf.UninterpretedOption"},{name:"declaration",number:2,type:11,label:3,typeName:".google.protobuf.ExtensionRangeOptions.Declaration",options:{retention:2}},{name:"features",number:50,type:11,label:1,typeName:".google.protobuf.FeatureSet"},{name:"verification",number:3,type:14,label:1,typeName:".google.protobuf.ExtensionRangeOptions.VerificationState",defaultValue:"UNVERIFIED",options:{retention:2}}],nestedType:[{name:"Declaration",field:[{name:"number",number:1,type:5,label:1},{name:"full_name",number:2,type:9,label:1},{name:"type",number:3,type:9,label:1},{name:"reserved",number:5,type:8,label:1},{name:"repeated",number:6,type:8,label:1}]}],enumType:[{name:"VerificationState",value:[{name:"DECLARATION",number:0},{name:"UNVERIFIED",number:1}]}],extensionRange:[{start:1e3,end:536870912}]},{name:"FieldDescriptorProto",field:[{name:"name",number:1,type:9,label:1},{name:"number",number:3,type:5,label:1},{name:"label",number:4,type:14,label:1,typeName:".google.protobuf.FieldDescriptorProto.Label"},{name:"type",number:5,type:14,label:1,typeName:".google.protobuf.FieldDescriptorProto.Type"},{name:"type_name",number:6,type:9,label:1},{name:"extendee",number:2,type:9,label:1},{name:"default_value",number:7,type:9,label:1},{name:"oneof_index",number:9,type:5,label:1},{name:"json_name",number:10,type:9,label:1},{name:"options",number:8,type:11,label:1,typeName:".google.protobuf.FieldOptions"},{name:"proto3_optional",number:17,type:8,label:1}],enumType:[{name:"Type",value:[{name:"TYPE_DOUBLE",number:1},{name:"TYPE_FLOAT",number:2},{name:"TYPE_INT64",number:3},{name:"TYPE_UINT64",number:4},{name:"TYPE_INT32",number:5},{name:"TYPE_FIXED64",number:6},{name:"TYPE_FIXED32",number:7},{name:"TYPE_BOOL",number:8},{name:"TYPE_STRING",number:9},{name:"TYPE_GROUP",number:10},{name:"TYPE_MESSAGE",number:11},{name:"TYPE_BYTES",number:12},{name:"TYPE_UINT32",number:13},{name:"TYPE_ENUM",number:14},{name:"TYPE_SFIXED32",number:15},{name:"TYPE_SFIXED64",number:16},{name:"TYPE_SINT32",number:17},{name:"TYPE_SINT64",number:18}]},{name:"Label",value:[{name:"LABEL_OPTIONAL",number:1},{name:"LABEL_REPEATED",number:3},{name:"LABEL_REQUIRED",number:2}]}]},{name:"OneofDescriptorProto",field:[{name:"name",number:1,type:9,label:1},{name:"options",number:2,type:11,label:1,typeName:".google.protobuf.OneofOptions"}]},{name:"EnumDescriptorProto",field:[{name:"name",number:1,type:9,label:1},{name:"value",number:2,type:11,label:3,typeName:".google.protobuf.EnumValueDescriptorProto"},{name:"options",number:3,type:11,label:1,typeName:".google.protobuf.EnumOptions"},{name:"reserved_range",number:4,type:11,label:3,typeName:".google.protobuf.EnumDescriptorProto.EnumReservedRange"},{name:"reserved_name",number:5,type:9,label:3},{name:"visibility",number:6,type:14,label:1,typeName:".google.protobuf.SymbolVisibility"}],nestedType:[{name:"EnumReservedRange",field:[{name:"start",number:1,type:5,label:1},{name:"end",number:2,type:5,label:1}]}]},{name:"EnumValueDescriptorProto",field:[{name:"name",number:1,type:9,label:1},{name:"number",number:2,type:5,label:1},{name:"options",number:3,type:11,label:1,typeName:".google.protobuf.EnumValueOptions"}]},{name:"ServiceDescriptorProto",field:[{name:"name",number:1,type:9,label:1},{name:"method",number:2,type:11,label:3,typeName:".google.protobuf.MethodDescriptorProto"},{name:"options",number:3,type:11,label:1,typeName:".google.protobuf.ServiceOptions"}]},{name:"MethodDescriptorProto",field:[{name:"name",number:1,type:9,label:1},{name:"input_type",number:2,type:9,label:1},{name:"output_type",number:3,type:9,label:1},{name:"options",number:4,type:11,label:1,typeName:".google.protobuf.MethodOptions"},{name:"client_streaming",number:5,type:8,label:1,defaultValue:"false"},{name:"server_streaming",number:6,type:8,label:1,defaultValue:"false"}]},{name:"FileOptions",field:[{name:"java_package",number:1,type:9,label:1},{name:"java_outer_classname",number:8,type:9,label:1},{name:"java_multiple_files",number:10,type:8,label:1,defaultValue:"false",options:{}},{name:"java_generate_equals_and_hash",number:20,type:8,label:1,options:{deprecated:!0}},{name:"java_string_check_utf8",number:27,type:8,label:1,defaultValue:"false"},{name:"optimize_for",number:9,type:14,label:1,typeName:".google.protobuf.FileOptions.OptimizeMode",defaultValue:"SPEED"},{name:"go_package",number:11,type:9,label:1},{name:"cc_generic_services",number:16,type:8,label:1,defaultValue:"false"},{name:"java_generic_services",number:17,type:8,label:1,defaultValue:"false"},{name:"py_generic_services",number:18,type:8,label:1,defaultValue:"false"},{name:"deprecated",number:23,type:8,label:1,defaultValue:"false"},{name:"cc_enable_arenas",number:31,type:8,label:1,defaultValue:"true"},{name:"objc_class_prefix",number:36,type:9,label:1},{name:"csharp_namespace",number:37,type:9,label:1},{name:"swift_prefix",number:39,type:9,label:1},{name:"php_class_prefix",number:40,type:9,label:1},{name:"php_namespace",number:41,type:9,label:1},{name:"php_metadata_namespace",number:44,type:9,label:1},{name:"ruby_package",number:45,type:9,label:1},{name:"features",number:50,type:11,label:1,typeName:".google.protobuf.FeatureSet"},{name:"uninterpreted_option",number:999,type:11,label:3,typeName:".google.protobuf.UninterpretedOption"}],enumType:[{name:"OptimizeMode",value:[{name:"SPEED",number:1},{name:"CODE_SIZE",number:2},{name:"LITE_RUNTIME",number:3}]}],extensionRange:[{start:1e3,end:536870912}]},{name:"MessageOptions",field:[{name:"message_set_wire_format",number:1,type:8,label:1,defaultValue:"false"},{name:"no_standard_descriptor_accessor",number:2,type:8,label:1,defaultValue:"false"},{name:"deprecated",number:3,type:8,label:1,defaultValue:"false"},{name:"map_entry",number:7,type:8,label:1},{name:"deprecated_legacy_json_field_conflicts",number:11,type:8,label:1,options:{deprecated:!0}},{name:"features",number:12,type:11,label:1,typeName:".google.protobuf.FeatureSet"},{name:"uninterpreted_option",number:999,type:11,label:3,typeName:".google.protobuf.UninterpretedOption"}],extensionRange:[{start:1e3,end:536870912}]},{name:"FieldOptions",field:[{name:"ctype",number:1,type:14,label:1,typeName:".google.protobuf.FieldOptions.CType",defaultValue:"STRING"},{name:"packed",number:2,type:8,label:1},{name:"jstype",number:6,type:14,label:1,typeName:".google.protobuf.FieldOptions.JSType",defaultValue:"JS_NORMAL"},{name:"lazy",number:5,type:8,label:1,defaultValue:"false"},{name:"unverified_lazy",number:15,type:8,label:1,defaultValue:"false"},{name:"deprecated",number:3,type:8,label:1,defaultValue:"false"},{name:"weak",number:10,type:8,label:1,defaultValue:"false",options:{deprecated:!0}},{name:"debug_redact",number:16,type:8,label:1,defaultValue:"false"},{name:"retention",number:17,type:14,label:1,typeName:".google.protobuf.FieldOptions.OptionRetention"},{name:"targets",number:19,type:14,label:3,typeName:".google.protobuf.FieldOptions.OptionTargetType"},{name:"edition_defaults",number:20,type:11,label:3,typeName:".google.protobuf.FieldOptions.EditionDefault"},{name:"features",number:21,type:11,label:1,typeName:".google.protobuf.FeatureSet"},{name:"feature_support",number:22,type:11,label:1,typeName:".google.protobuf.FieldOptions.FeatureSupport"},{name:"uninterpreted_option",number:999,type:11,label:3,typeName:".google.protobuf.UninterpretedOption"}],nestedType:[{name:"EditionDefault",field:[{name:"edition",number:3,type:14,label:1,typeName:".google.protobuf.Edition"},{name:"value",number:2,type:9,label:1}]},{name:"FeatureSupport",field:[{name:"edition_introduced",number:1,type:14,label:1,typeName:".google.protobuf.Edition"},{name:"edition_deprecated",number:2,type:14,label:1,typeName:".google.protobuf.Edition"},{name:"deprecation_warning",number:3,type:9,label:1},{name:"edition_removed",number:4,type:14,label:1,typeName:".google.protobuf.Edition"},{name:"removal_error",number:5,type:9,label:1}]}],enumType:[{name:"CType",value:[{name:"STRING",number:0},{name:"CORD",number:1},{name:"STRING_PIECE",number:2}]},{name:"JSType",value:[{name:"JS_NORMAL",number:0},{name:"JS_STRING",number:1},{name:"JS_NUMBER",number:2}]},{name:"OptionRetention",value:[{name:"RETENTION_UNKNOWN",number:0},{name:"RETENTION_RUNTIME",number:1},{name:"RETENTION_SOURCE",number:2}]},{name:"OptionTargetType",value:[{name:"TARGET_TYPE_UNKNOWN",number:0},{name:"TARGET_TYPE_FILE",number:1},{name:"TARGET_TYPE_EXTENSION_RANGE",number:2},{name:"TARGET_TYPE_MESSAGE",number:3},{name:"TARGET_TYPE_FIELD",number:4},{name:"TARGET_TYPE_ONEOF",number:5},{name:"TARGET_TYPE_ENUM",number:6},{name:"TARGET_TYPE_ENUM_ENTRY",number:7},{name:"TARGET_TYPE_SERVICE",number:8},{name:"TARGET_TYPE_METHOD",number:9}]}],extensionRange:[{start:1e3,end:536870912}]},{name:"OneofOptions",field:[{name:"features",number:1,type:11,label:1,typeName:".google.protobuf.FeatureSet"},{name:"uninterpreted_option",number:999,type:11,label:3,typeName:".google.protobuf.UninterpretedOption"}],extensionRange:[{start:1e3,end:536870912}]},{name:"EnumOptions",field:[{name:"allow_alias",number:2,type:8,label:1},{name:"deprecated",number:3,type:8,label:1,defaultValue:"false"},{name:"deprecated_legacy_json_field_conflicts",number:6,type:8,label:1,options:{deprecated:!0}},{name:"features",number:7,type:11,label:1,typeName:".google.protobuf.FeatureSet"},{name:"uninterpreted_option",number:999,type:11,label:3,typeName:".google.protobuf.UninterpretedOption"}],extensionRange:[{start:1e3,end:536870912}]},{name:"EnumValueOptions",field:[{name:"deprecated",number:1,type:8,label:1,defaultValue:"false"},{name:"features",number:2,type:11,label:1,typeName:".google.protobuf.FeatureSet"},{name:"debug_redact",number:3,type:8,label:1,defaultValue:"false"},{name:"feature_support",number:4,type:11,label:1,typeName:".google.protobuf.FieldOptions.FeatureSupport"},{name:"uninterpreted_option",number:999,type:11,label:3,typeName:".google.protobuf.UninterpretedOption"}],extensionRange:[{start:1e3,end:536870912}]},{name:"ServiceOptions",field:[{name:"features",number:34,type:11,label:1,typeName:".google.protobuf.FeatureSet"},{name:"deprecated",number:33,type:8,label:1,defaultValue:"false"},{name:"uninterpreted_option",number:999,type:11,label:3,typeName:".google.protobuf.UninterpretedOption"}],extensionRange:[{start:1e3,end:536870912}]},{name:"MethodOptions",field:[{name:"deprecated",number:33,type:8,label:1,defaultValue:"false"},{name:"idempotency_level",number:34,type:14,label:1,typeName:".google.protobuf.MethodOptions.IdempotencyLevel",defaultValue:"IDEMPOTENCY_UNKNOWN"},{name:"features",number:35,type:11,label:1,typeName:".google.protobuf.FeatureSet"},{name:"uninterpreted_option",number:999,type:11,label:3,typeName:".google.protobuf.UninterpretedOption"}],enumType:[{name:"IdempotencyLevel",value:[{name:"IDEMPOTENCY_UNKNOWN",number:0},{name:"NO_SIDE_EFFECTS",number:1},{name:"IDEMPOTENT",number:2}]}],extensionRange:[{start:1e3,end:536870912}]},{name:"UninterpretedOption",field:[{name:"name",number:2,type:11,label:3,typeName:".google.protobuf.UninterpretedOption.NamePart"},{name:"identifier_value",number:3,type:9,label:1},{name:"positive_int_value",number:4,type:4,label:1},{name:"negative_int_value",number:5,type:3,label:1},{name:"double_value",number:6,type:1,label:1},{name:"string_value",number:7,type:12,label:1},{name:"aggregate_value",number:8,type:9,label:1}],nestedType:[{name:"NamePart",field:[{name:"name_part",number:1,type:9,label:2},{name:"is_extension",number:2,type:8,label:2}]}]},{name:"FeatureSet",field:[{name:"field_presence",number:1,type:14,label:1,typeName:".google.protobuf.FeatureSet.FieldPresence",options:{retention:1,targets:[4,1],editionDefaults:[{value:"EXPLICIT",edition:900},{value:"IMPLICIT",edition:999},{value:"EXPLICIT",edition:1e3}]}},{name:"enum_type",number:2,type:14,label:1,typeName:".google.protobuf.FeatureSet.EnumType",options:{retention:1,targets:[6,1],editionDefaults:[{value:"CLOSED",edition:900},{value:"OPEN",edition:999}]}},{name:"repeated_field_encoding",number:3,type:14,label:1,typeName:".google.protobuf.FeatureSet.RepeatedFieldEncoding",options:{retention:1,targets:[4,1],editionDefaults:[{value:"EXPANDED",edition:900},{value:"PACKED",edition:999}]}},{name:"utf8_validation",number:4,type:14,label:1,typeName:".google.protobuf.FeatureSet.Utf8Validation",options:{retention:1,targets:[4,1],editionDefaults:[{value:"NONE",edition:900},{value:"VERIFY",edition:999}]}},{name:"message_encoding",number:5,type:14,label:1,typeName:".google.protobuf.FeatureSet.MessageEncoding",options:{retention:1,targets:[4,1],editionDefaults:[{value:"LENGTH_PREFIXED",edition:900}]}},{name:"json_format",number:6,type:14,label:1,typeName:".google.protobuf.FeatureSet.JsonFormat",options:{retention:1,targets:[3,6,1],editionDefaults:[{value:"LEGACY_BEST_EFFORT",edition:900},{value:"ALLOW",edition:999}]}},{name:"enforce_naming_style",number:7,type:14,label:1,typeName:".google.protobuf.FeatureSet.EnforceNamingStyle",options:{retention:2,targets:[1,2,3,4,5,6,7,8,9],editionDefaults:[{value:"STYLE_LEGACY",edition:900},{value:"STYLE2024",edition:1001}]}},{name:"default_symbol_visibility",number:8,type:14,label:1,typeName:".google.protobuf.FeatureSet.VisibilityFeature.DefaultSymbolVisibility",options:{retention:2,targets:[1],editionDefaults:[{value:"EXPORT_ALL",edition:900},{value:"EXPORT_TOP_LEVEL",edition:1001}]}}],nestedType:[{name:"VisibilityFeature",enumType:[{name:"DefaultSymbolVisibility",value:[{name:"DEFAULT_SYMBOL_VISIBILITY_UNKNOWN",number:0},{name:"EXPORT_ALL",number:1},{name:"EXPORT_TOP_LEVEL",number:2},{name:"LOCAL_ALL",number:3},{name:"STRICT",number:4}]}]}],enumType:[{name:"FieldPresence",value:[{name:"FIELD_PRESENCE_UNKNOWN",number:0},{name:"EXPLICIT",number:1},{name:"IMPLICIT",number:2},{name:"LEGACY_REQUIRED",number:3}]},{name:"EnumType",value:[{name:"ENUM_TYPE_UNKNOWN",number:0},{name:"OPEN",number:1},{name:"CLOSED",number:2}]},{name:"RepeatedFieldEncoding",value:[{name:"REPEATED_FIELD_ENCODING_UNKNOWN",number:0},{name:"PACKED",number:1},{name:"EXPANDED",number:2}]},{name:"Utf8Validation",value:[{name:"UTF8_VALIDATION_UNKNOWN",number:0},{name:"VERIFY",number:2},{name:"NONE",number:3}]},{name:"MessageEncoding",value:[{name:"MESSAGE_ENCODING_UNKNOWN",number:0},{name:"LENGTH_PREFIXED",number:1},{name:"DELIMITED",number:2}]},{name:"JsonFormat",value:[{name:"JSON_FORMAT_UNKNOWN",number:0},{name:"ALLOW",number:1},{name:"LEGACY_BEST_EFFORT",number:2}]},{name:"EnforceNamingStyle",value:[{name:"ENFORCE_NAMING_STYLE_UNKNOWN",number:0},{name:"STYLE2024",number:1},{name:"STYLE_LEGACY",number:2}]}],extensionRange:[{start:1e3,end:9995},{start:9995,end:1e4},{start:1e4,end:10001}]},{name:"FeatureSetDefaults",field:[{name:"defaults",number:1,type:11,label:3,typeName:".google.protobuf.FeatureSetDefaults.FeatureSetEditionDefault"},{name:"minimum_edition",number:4,type:14,label:1,typeName:".google.protobuf.Edition"},{name:"maximum_edition",number:5,type:14,label:1,typeName:".google.protobuf.Edition"}],nestedType:[{name:"FeatureSetEditionDefault",field:[{name:"edition",number:3,type:14,label:1,typeName:".google.protobuf.Edition"},{name:"overridable_features",number:4,type:11,label:1,typeName:".google.protobuf.FeatureSet"},{name:"fixed_features",number:5,type:11,label:1,typeName:".google.protobuf.FeatureSet"}]}]},{name:"SourceCodeInfo",field:[{name:"location",number:1,type:11,label:3,typeName:".google.protobuf.SourceCodeInfo.Location"}],nestedType:[{name:"Location",field:[{name:"path",number:1,type:5,label:3,options:{packed:!0}},{name:"span",number:2,type:5,label:3,options:{packed:!0}},{name:"leading_comments",number:3,type:9,label:1},{name:"trailing_comments",number:4,type:9,label:1},{name:"leading_detached_comments",number:6,type:9,label:3}]}],extensionRange:[{start:536e6,end:536000001}]},{name:"GeneratedCodeInfo",field:[{name:"annotation",number:1,type:11,label:3,typeName:".google.protobuf.GeneratedCodeInfo.Annotation"}],nestedType:[{name:"Annotation",field:[{name:"path",number:1,type:5,label:3,options:{packed:!0}},{name:"source_file",number:2,type:9,label:1},{name:"begin",number:3,type:5,label:1},{name:"end",number:4,type:5,label:1},{name:"semantic",number:5,type:14,label:1,typeName:".google.protobuf.GeneratedCodeInfo.Annotation.Semantic"}],enumType:[{name:"Semantic",value:[{name:"NONE",number:0},{name:"SET",number:1},{name:"ALIAS",number:2}]}]}]}],enumType:[{name:"Edition",value:[{name:"EDITION_UNKNOWN",number:0},{name:"EDITION_LEGACY",number:900},{name:"EDITION_PROTO2",number:998},{name:"EDITION_PROTO3",number:999},{name:"EDITION_2023",number:1e3},{name:"EDITION_2024",number:1001},{name:"EDITION_UNSTABLE",number:9999},{name:"EDITION_1_TEST_ONLY",number:1},{name:"EDITION_2_TEST_ONLY",number:2},{name:"EDITION_99997_TEST_ONLY",number:99997},{name:"EDITION_99998_TEST_ONLY",number:99998},{name:"EDITION_99999_TEST_ONLY",number:99999},{name:"EDITION_MAX",number:2147483647}]},{name:"SymbolVisibility",value:[{name:"VISIBILITY_UNSET",number:0},{name:"VISIBILITY_LOCAL",number:1},{name:"VISIBILITY_EXPORT",number:2}]}]});var Pg=qi(pd,1);var xg;(function(n){n[n.DECLARATION=0]="DECLARATION",n[n.UNVERIFIED=1]="UNVERIFIED"})(xg||(xg={}));var yg;(function(n){n[n.DOUBLE=1]="DOUBLE",n[n.FLOAT=2]="FLOAT",n[n.INT64=3]="INT64",n[n.UINT64=4]="UINT64",n[n.INT32=5]="INT32",n[n.FIXED64=6]="FIXED64",n[n.FIXED32=7]="FIXED32",n[n.BOOL=8]="BOOL",n[n.STRING=9]="STRING",n[n.GROUP=10]="GROUP",n[n.MESSAGE=11]="MESSAGE",n[n.BYTES=12]="BYTES",n[n.UINT32=13]="UINT32",n[n.ENUM=14]="ENUM",n[n.SFIXED32=15]="SFIXED32",n[n.SFIXED64=16]="SFIXED64",n[n.SINT32=17]="SINT32",n[n.SINT64=18]="SINT64"})(yg||(yg={}));var _g;(function(n){n[n.OPTIONAL=1]="OPTIONAL",n[n.REPEATED=3]="REPEATED",n[n.REQUIRED=2]="REQUIRED"})(_g||(_g={}));var bg;(function(n){n[n.SPEED=1]="SPEED",n[n.CODE_SIZE=2]="CODE_SIZE",n[n.LITE_RUNTIME=3]="LITE_RUNTIME"})(bg||(bg={}));var Sg;(function(n){n[n.STRING=0]="STRING",n[n.CORD=1]="CORD",n[n.STRING_PIECE=2]="STRING_PIECE"})(Sg||(Sg={}));var vg;(function(n){n[n.JS_NORMAL=0]="JS_NORMAL",n[n.JS_STRING=1]="JS_STRING",n[n.JS_NUMBER=2]="JS_NUMBER"})(vg||(vg={}));var Ag;(function(n){n[n.RETENTION_UNKNOWN=0]="RETENTION_UNKNOWN",n[n.RETENTION_RUNTIME=1]="RETENTION_RUNTIME",n[n.RETENTION_SOURCE=2]="RETENTION_SOURCE"})(Ag||(Ag={}));var Tg;(function(n){n[n.TARGET_TYPE_UNKNOWN=0]="TARGET_TYPE_UNKNOWN",n[n.TARGET_TYPE_FILE=1]="TARGET_TYPE_FILE",n[n.TARGET_TYPE_EXTENSION_RANGE=2]="TARGET_TYPE_EXTENSION_RANGE",n[n.TARGET_TYPE_MESSAGE=3]="TARGET_TYPE_MESSAGE",n[n.TARGET_TYPE_FIELD=4]="TARGET_TYPE_FIELD",n[n.TARGET_TYPE_ONEOF=5]="TARGET_TYPE_ONEOF",n[n.TARGET_TYPE_ENUM=6]="TARGET_TYPE_ENUM",n[n.TARGET_TYPE_ENUM_ENTRY=7]="TARGET_TYPE_ENUM_ENTRY",n[n.TARGET_TYPE_SERVICE=8]="TARGET_TYPE_SERVICE",n[n.TARGET_TYPE_METHOD=9]="TARGET_TYPE_METHOD"})(Tg||(Tg={}));var Eg;(function(n){n[n.IDEMPOTENCY_UNKNOWN=0]="IDEMPOTENCY_UNKNOWN",n[n.NO_SIDE_EFFECTS=1]="NO_SIDE_EFFECTS",n[n.IDEMPOTENT=2]="IDEMPOTENT"})(Eg||(Eg={}));var Mg;(function(n){n[n.DEFAULT_SYMBOL_VISIBILITY_UNKNOWN=0]="DEFAULT_SYMBOL_VISIBILITY_UNKNOWN",n[n.EXPORT_ALL=1]="EXPORT_ALL",n[n.EXPORT_TOP_LEVEL=2]="EXPORT_TOP_LEVEL",n[n.LOCAL_ALL=3]="LOCAL_ALL",n[n.STRICT=4]="STRICT"})(Mg||(Mg={}));var Rg;(function(n){n[n.FIELD_PRESENCE_UNKNOWN=0]="FIELD_PRESENCE_UNKNOWN",n[n.EXPLICIT=1]="EXPLICIT",n[n.IMPLICIT=2]="IMPLICIT",n[n.LEGACY_REQUIRED=3]="LEGACY_REQUIRED"})(Rg||(Rg={}));var wg;(function(n){n[n.ENUM_TYPE_UNKNOWN=0]="ENUM_TYPE_UNKNOWN",n[n.OPEN=1]="OPEN",n[n.CLOSED=2]="CLOSED"})(wg||(wg={}));var Cg;(function(n){n[n.REPEATED_FIELD_ENCODING_UNKNOWN=0]="REPEATED_FIELD_ENCODING_UNKNOWN",n[n.PACKED=1]="PACKED",n[n.EXPANDED=2]="EXPANDED"})(Cg||(Cg={}));var Ng;(function(n){n[n.UTF8_VALIDATION_UNKNOWN=0]="UTF8_VALIDATION_UNKNOWN",n[n.VERIFY=2]="VERIFY",n[n.NONE=3]="NONE"})(Ng||(Ng={}));var Ig;(function(n){n[n.MESSAGE_ENCODING_UNKNOWN=0]="MESSAGE_ENCODING_UNKNOWN",n[n.LENGTH_PREFIXED=1]="LENGTH_PREFIXED",n[n.DELIMITED=2]="DELIMITED"})(Ig||(Ig={}));var Bg;(function(n){n[n.JSON_FORMAT_UNKNOWN=0]="JSON_FORMAT_UNKNOWN",n[n.ALLOW=1]="ALLOW",n[n.LEGACY_BEST_EFFORT=2]="LEGACY_BEST_EFFORT"})(Bg||(Bg={}));var Fg;(function(n){n[n.ENFORCE_NAMING_STYLE_UNKNOWN=0]="ENFORCE_NAMING_STYLE_UNKNOWN",n[n.STYLE2024=1]="STYLE2024",n[n.STYLE_LEGACY=2]="STYLE_LEGACY"})(Fg||(Fg={}));var Lg;(function(n){n[n.NONE=0]="NONE",n[n.SET=1]="SET",n[n.ALIAS=2]="ALIAS"})(Lg||(Lg={}));var Ug;(function(n){n[n.EDITION_UNKNOWN=0]="EDITION_UNKNOWN",n[n.EDITION_LEGACY=900]="EDITION_LEGACY",n[n.EDITION_PROTO2=998]="EDITION_PROTO2",n[n.EDITION_PROTO3=999]="EDITION_PROTO3",n[n.EDITION_2023=1e3]="EDITION_2023",n[n.EDITION_2024=1001]="EDITION_2024",n[n.EDITION_UNSTABLE=9999]="EDITION_UNSTABLE",n[n.EDITION_1_TEST_ONLY=1]="EDITION_1_TEST_ONLY",n[n.EDITION_2_TEST_ONLY=2]="EDITION_2_TEST_ONLY",n[n.EDITION_99997_TEST_ONLY=99997]="EDITION_99997_TEST_ONLY",n[n.EDITION_99998_TEST_ONLY=99998]="EDITION_99998_TEST_ONLY",n[n.EDITION_99999_TEST_ONLY=99999]="EDITION_99999_TEST_ONLY",n[n.EDITION_MAX=2147483647]="EDITION_MAX"})(Ug||(Ug={}));var Dg;(function(n){n[n.VISIBILITY_UNSET=0]="VISIBILITY_UNSET",n[n.VISIBILITY_LOCAL=1]="VISIBILITY_LOCAL",n[n.VISIBILITY_EXPORT=2]="VISIBILITY_EXPORT"})(Dg||(Dg={}));function lE(n){return Object.assign(Object.assign({readUnknownFields:!0,recursionLimit:100},n),{depth:0})}function Ma(n,e,t){let i=Bs(n);return md(n).read(i,new Ec(e),lE(t),e.byteLength),i}var Gg=new WeakMap;function md(n){let e=Gg.get(n);return e===void 0&&(e=cE(n)),e}function cE(n){let e=String(n),t=new Map,i={read:uE(e,t),readGroup:hE(e,t)};Gg.set(n,i);for(let s of n.fields)t.set(s.number,dE(s));return i}function uE(n,e){return(t,i,s,r)=>{var o;if(++s.depth>s.recursionLimit)throw new Error(`cannot decode ${n} from binary: maximum recursion depth of ${s.recursionLimit} reached`);let a=i.pos+r,l=(o=t.$unknown)!==null&&o!==void 0?o:[];for(;i.pos<a;){let[c,u]=i.tag(),d=e.get(c);if(d===void 0){let h=i.skip(u,c,s.recursionLimit-s.depth);s.readUnknownFields&&l.push({no:c,wireType:u,data:h});continue}d(t,i,s,u)}l.length>0&&(t.$unknown=l),s.depth--}}function hE(n,e){return(t,i,s,r)=>{var o;if(++s.depth>s.recursionLimit)throw new Error(`cannot decode ${n} from binary: maximum recursion depth of ${s.recursionLimit} reached`);let a,l,c=(o=t.$unknown)!==null&&o!==void 0?o:[];for(;i.pos<i.len&&([a,l]=i.tag(),l!=Et.EndGroup);){let u=e.get(a);if(u===void 0){let d=i.skip(l,a,s.recursionLimit-s.depth);s.readUnknownFields&&c.push({no:a,wireType:l,data:d});continue}u(t,i,s,l)}if(l!=Et.EndGroup||a!==r)throw new Error("invalid end group tag");c.length>0&&(t.$unknown=c),s.depth--}}function dE(n){switch(n.fieldKind){case"scalar":return fE(n);case"enum":return pE(n);case"message":return mE(n);case"list":return gE(n);case"map":return xE(n)}}function fE(n){let e=Nc(n.scalar,n.utf8Validation,n.longAsString),t=n.localName;if(n.oneof){let i=n.oneof.localName;return(s,r)=>{s[i]={case:t,value:e(r)}}}return(i,s)=>{i[t]=e(s)}}function pE(n){var e;let t=n.localName,i=(e=n.oneof)===null||e===void 0?void 0:e.localName;if(n.enum.open)return i!==void 0?(o,a)=>{o[i]={case:t,value:a.int32()}}:(o,a)=>{o[t]=a.int32()};let s=n.enum.values,r=n.number;return(o,a,l,c)=>{var u;let d=a.int32();if(s.some(h=>h.number===d))i!==void 0?o[i]={case:t,value:d}:o[t]=d;else if(l.readUnknownFields){let h=[];Gm(d,h);let f=(u=o.$unknown)!==null&&u!==void 0?u:[];f.push({no:r,wireType:c,data:new Uint8Array(h)}),o.$unknown=f}}}function mE(n){let e=n.localName,{toMessage:t,toLocal:i}=Ji(n),s=Vg(n);if(n.oneof){let r=n.oneof.localName;return(o,a,l)=>{let c=o[r],u=t(c.case===e?c.value:void 0);s(u,a,l),o[r]={case:e,value:i(u)}}}return(r,o,a)=>{let l=t(r[e]);s(l,o,a),r[e]=i(l)}}function Vg(n){let e=md(n.message);if(n.delimitedEncoding){let t=n.number;return(i,s,r)=>e.readGroup(i,s,r,t)}return(t,i,s)=>e.read(t,i,s,i.uint32())}function gE(n){let e=n.localName;if(n.listKind=="message"){let{toMessage:o,toLocal:a}=Ji(n),l=Vg(n);return(c,u,d)=>{let h=o(void 0);l(h,u,d),c[e].push(a(h))}}let t=n.listKind=="enum"?oe.INT32:n.scalar,i=n.listKind=="scalar"?n.longAsString:!1,s=Nc(t,n.utf8Validation,i),r=t!=oe.STRING&&t!=oe.BYTES;return(o,a,l,c)=>{let u=o[e];if(c==Et.LengthDelimited&&r){let d=a.uint32()+a.pos;for(;a.pos<d;)u.push(s(a))}else u.push(s(a))}}function xE(n){let e=n.localName,t=Nc(n.mapKey,n.utf8Validation,!1),i=_a(n.mapKey,!1),s,r;switch(n.mapKind){case"scalar":{let o=n.scalar,a=Nc(o,n.utf8Validation,!1);if(s=l=>a(l),o==oe.BYTES)r=()=>new Uint8Array(0);else{let l=_a(o,!1);r=()=>l}break}case"enum":{let o=n.enum.values[0].number;s=a=>a.int32(),r=()=>o;break}case"message":{let{toMessage:o,toLocal:a}=Ji(n),l=md(n.message).read;s=(c,u)=>{let d=o(void 0);return l(d,c,u,c.uint32()),a(d)},r=()=>a(o(void 0));break}}return(o,a,l)=>{let c=o[e],u,d,h=a.uint32(),f=a.pos+h;for(;a.pos<f;){let[g]=a.tag();switch(g){case 1:u=t(a);break;case 2:d=s(a,l);break}}u===void 0&&(u=i),d===void 0&&(d=r()),c[u]=d}}function Nc(n,e,t){switch(n){case oe.STRING:return i=>i.string(e);case oe.BOOL:return i=>i.bool();case oe.DOUBLE:return i=>i.double();case oe.FLOAT:return i=>i.float();case oe.INT32:return i=>i.int32();case oe.INT64:return t?i=>String(i.int64()):i=>i.int64();case oe.UINT64:return t?i=>String(i.uint64()):i=>i.uint64();case oe.FIXED64:return t?i=>String(i.fixed64()):i=>i.fixed64();case oe.BYTES:return i=>i.bytes();case oe.FIXED32:return i=>i.fixed32();case oe.SFIXED32:return i=>i.sfixed32();case oe.SFIXED64:return t?i=>String(i.sfixed64()):i=>i.sfixed64();case oe.SINT64:return t?i=>String(i.sint64()):i=>i.sint64();case oe.UINT32:return i=>i.uint32();case oe.SINT32:return i=>i.sint32()}}function on(n,e){var t;let i=Ma(Pg,ig(n));return i.messageType.forEach(Aa),i.dependency=(t=e?.map(r=>r.proto.name))!==null&&t!==void 0?t:[],wc(i,r=>e?.find(o=>o.proto.name===r)).getFile(i.name)}var Og=on("Ch9nb29nbGUvcHJvdG9idWYvdGltZXN0YW1wLnByb3RvEg9nb29nbGUucHJvdG9idWYiKwoJVGltZXN0YW1wEg8KB3NlY29uZHMYASABKAMSDQoFbmFub3MYAiABKAVChQEKE2NvbS5nb29nbGUucHJvdG9idWZCDlRpbWVzdGFtcFByb3RvUAFaMmdvb2dsZS5nb2xhbmcub3JnL3Byb3RvYnVmL3R5cGVzL2tub3duL3RpbWVzdGFtcHBi+AEBogIDR1BCqgIeR29vZ2xlLlByb3RvYnVmLldlbGxLbm93blR5cGVzYgZwcm90bzM");var yE=2,_E=3,kg={writeUnknownFields:!0};function bE(n){return n?Object.assign(Object.assign({},kg),n):kg}function gd(n,e,t){let i=new Tc;return xd(n)(i,bE(t),e),i.finish()}var zg=new WeakMap;function xd(n){let e=zg.get(n);return e===void 0&&(e=SE(n)),e}function SE(n){let e=n.typeName,t=n.fields.concat().sort((o,a)=>o.number-a.number),i=t[0],s=[],r=(o,a,l)=>{if(l.$typeName!==e&&i!==void 0)throw new Ac(i,`cannot use ${i} with message ${l.$typeName}`,"ForeignFieldError");for(let u=0;u<s.length;u++)s[u](o,a,l);let c=l.$unknown;if(c!==void 0&&a.writeUnknownFields)for(let u=0;u<c.length;u++){let{no:d,wireType:h,data:f}=c[u];o.tag(d,h).raw(f)}};zg.set(n,r);for(let o of t)s.push(vE(o));return r}function vE(n){switch(n.fieldKind){case"message":case"scalar":case"enum":return AE(n);case"list":return EE(n);case"map":return ME(n)}}function AE(n){let e=TE(n),t=n.localName;if(n.oneof){let i=n.oneof.localName;return(s,r,o)=>{let a=o[i];a.case===t&&e(s,r,a.value)}}if(n.presence!=yE){let i=n.presence==_E?`cannot encode ${n} to binary: required field not set`:void 0;return(s,r,o)=>{let a=o[t];if(a!==void 0&&Object.prototype.hasOwnProperty.call(o,t))e(s,r,a);else if(i!==void 0)throw new Error(i)}}if(n.fieldKind=="enum"){let i=n.enum.values[0].number;return(s,r,o)=>{let a=o[t];a!==i&&e(s,r,a)}}switch(n.scalar){case oe.BOOL:return(i,s,r)=>{let o=r[t];o!==!1&&e(i,s,o)};case oe.STRING:return(i,s,r)=>{let o=r[t];o!==""&&e(i,s,o)};case oe.BYTES:return(i,s,r)=>{let o=r[t];(!(o instanceof Uint8Array)||o.byteLength>0)&&e(i,s,o)};case oe.DOUBLE:case oe.FLOAT:return(i,s,r)=>{let o=r[t];Object.is(o,0)||e(i,s,o)};default:return(i,s,r)=>{let o=r[t];o!=0&&e(i,s,o)}}}function TE(n){switch(n.fieldKind){case"message":{let{toMessage:e}=Ji(n),t=Wg(n);return(i,s,r)=>{t(i,s,e(r))}}case"scalar":case"enum":{let e=n.fieldKind=="enum"?oe.INT32:n.scalar,t=n.number,i=Bc(e),s=Ic(e,n.parent.typeName,n.name);return(r,o,a)=>{r.tag(t,i),s(r,a)}}}}function EE(n){let e=n.localName,t=n.number;switch(n.listKind){case"message":{let{toMessage:i}=Ji(n),s=Wg(n);return(r,o,a)=>{let l=a[e];for(let c=0;c<l.length;c++)s(r,o,i(l[c]))}}case"scalar":case"enum":{let i=n.listKind=="enum"?oe.INT32:n.scalar,s=Ic(i,n.parent.typeName,n.name);if(n.packed)return(o,a,l)=>{let c=l[e];if(c.length!=0){o.tag(t,Et.LengthDelimited).fork();for(let u=0;u<c.length;u++)s(o,c[u]);o.join()}};let r=Bc(i);return(o,a,l)=>{let c=l[e];for(let u=0;u<c.length;u++)o.tag(t,r),s(o,c[u])}}}}function ME(n){let e=n.localName,t=n.number,i=RE(n);if(n.mapKind=="message"){let{toMessage:a}=Ji(n),l=xd(n.message);return(c,u,d)=>{let h=d[e],f=Object.keys(h);for(let g=0;g<f.length;g++){let y=f[g];c.tag(t,Et.LengthDelimited).fork(),i(c,y),c.tag(2,Et.LengthDelimited).fork(),l(c,u,a(h[y])),c.join(),c.join()}}}let s=n.mapKind=="enum"?oe.INT32:n.scalar,r=Bc(s),o=Ic(s,n.parent.typeName,n.name);return(a,l,c)=>{let u=c[e],d=Object.keys(u);for(let h=0;h<d.length;h++){let f=d[h];a.tag(t,Et.LengthDelimited).fork(),i(a,f),a.tag(2,r),o(a,u[f]),a.join()}}}function RE(n){let e=Bc(n.mapKey),t=Ic(n.mapKey,n.parent.typeName,n.name),i=wE(n.mapKey);return(s,r)=>{s.tag(1,e),t(s,i(r))}}function wE(n){switch(n){case oe.STRING:return e=>e;case oe.BOOL:return e=>e==="true"?!0:e==="false"?!1:e;case oe.UINT64:case oe.FIXED64:return e=>{try{return Pt.uParse(e)}catch{return e}};case oe.INT64:case oe.SFIXED64:case oe.SINT64:return e=>{try{return Pt.parse(e)}catch{return e}};default:return e=>{let t=Number.parseInt(e);return Number.isFinite(t)?t:e}}}function Ic(n,e,t){let i=CE(n);return(s,r)=>{try{i(s,r)}catch(o){throw o instanceof Error?new Error(`cannot encode field ${e}.${t} to binary: ${o.message}`):o}}}function CE(n){switch(n){case oe.STRING:return(e,t)=>e.string(t);case oe.BOOL:return(e,t)=>e.bool(t);case oe.DOUBLE:return(e,t)=>e.double(t);case oe.FLOAT:return(e,t)=>e.float(t);case oe.INT32:return(e,t)=>e.int32(t);case oe.INT64:return(e,t)=>e.int64(t);case oe.UINT64:return(e,t)=>e.uint64(t);case oe.FIXED64:return(e,t)=>e.fixed64(t);case oe.BYTES:return(e,t)=>e.bytes(t);case oe.FIXED32:return(e,t)=>e.fixed32(t);case oe.SFIXED32:return(e,t)=>e.sfixed32(t);case oe.SFIXED64:return(e,t)=>e.sfixed64(t);case oe.SINT64:return(e,t)=>e.sint64(t);case oe.UINT32:return(e,t)=>e.uint32(t);case oe.SINT32:return(e,t)=>e.sint32(t)}}function Wg(n){let e=n.number,t=xd(n.message);return n.delimitedEncoding?(i,s,r)=>{i.tag(e,Et.StartGroup),t(i,s,r),i.tag(e,Et.EndGroup)}:(i,s,r)=>{i.tag(e,Et.LengthDelimited).fork(),t(i,s,r),i.join()}}function Bc(n){switch(n){case oe.BYTES:case oe.STRING:return Et.LengthDelimited;case oe.DOUBLE:case oe.FIXED64:case oe.SFIXED64:return Et.Bit64;case oe.FIXED32:case oe.SFIXED32:case oe.FLOAT:return Et.Bit32;default:return Et.Varint}}var Yg=on("Chxnb29nbGUvcHJvdG9idWYvc3RydWN0LnByb3RvEg9nb29nbGUucHJvdG9idWYihAEKBlN0cnVjdBIzCgZmaWVsZHMYASADKAsyIy5nb29nbGUucHJvdG9idWYuU3RydWN0LkZpZWxkc0VudHJ5GkUKC0ZpZWxkc0VudHJ5EgsKA2tleRgBIAEoCRIlCgV2YWx1ZRgCIAEoCzIWLmdvb2dsZS5wcm90b2J1Zi5WYWx1ZToCOAEi6gEKBVZhbHVlEjAKCm51bGxfdmFsdWUYASABKA4yGi5nb29nbGUucHJvdG9idWYuTnVsbFZhbHVlSAASFgoMbnVtYmVyX3ZhbHVlGAIgASgBSAASFgoMc3RyaW5nX3ZhbHVlGAMgASgJSAASFAoKYm9vbF92YWx1ZRgEIAEoCEgAEi8KDHN0cnVjdF92YWx1ZRgFIAEoCzIXLmdvb2dsZS5wcm90b2J1Zi5TdHJ1Y3RIABIwCgpsaXN0X3ZhbHVlGAYgASgLMhouZ29vZ2xlLnByb3RvYnVmLkxpc3RWYWx1ZUgAQgYKBGtpbmQiMwoJTGlzdFZhbHVlEiYKBnZhbHVlcxgBIAMoCzIWLmdvb2dsZS5wcm90b2J1Zi5WYWx1ZSobCglOdWxsVmFsdWUSDgoKTlVMTF9WQUxVRRAAQn8KE2NvbS5nb29nbGUucHJvdG9idWZCC1N0cnVjdFByb3RvUAFaL2dvb2dsZS5nb2xhbmcub3JnL3Byb3RvYnVmL3R5cGVzL2tub3duL3N0cnVjdHBi+AEBogIDR1BCqgIeR29vZ2xlLlByb3RvYnVmLldlbGxLbm93blR5cGVzYgZwcm90bzM");var Hg;(function(n){n[n.NULL_VALUE=0]="NULL_VALUE"})(Hg||(Hg={}));function Zg(n,e,...t){if(t.length>0)throw new Error;return n.services[e]}var Qi=on("ChljYWRwb2xlL3YxL2dlb21ldHJ5LnByb3RvEgpjYWRwb2xlLnYxIhwKBFZlYzISCQoBeBgBIAEoARIJCgF5GAIgASgBIicKBFZlYzMSCQoBeBgBIAEoARIJCgF5GAIgASgBEgkKAXoYAyABKAEiMgoEUXVhdBIJCgF4GAEgASgBEgkKAXkYAiABKAESCQoBehgDIAEoARIJCgF3GAQgASgBIlYKCVRyYW5zZm9ybRIlCgt0cmFuc2xhdGlvbhgBIAEoCzIQLmNhZHBvbGUudjEuVmVjMxIiCghyb3RhdGlvbhgCIAEoCzIQLmNhZHBvbGUudjEuUXVhdCJsCgVQbGFuZRIgCgZvcmlnaW4YASABKAsyEC5jYWRwb2xlLnYxLlZlYzMSIAoGbm9ybWFsGAIgASgLMhAuY2FkcG9sZS52MS5WZWMzEh8KBXhfZGlyGAMgASgLMhAuY2FkcG9sZS52MS5WZWMzIkQKBEJCb3gSHQoDbWluGAEgASgLMhAuY2FkcG9sZS52MS5WZWMzEh0KA21heBgCIAEoCzIQLmNhZHBvbGUudjEuVmVjMyqMAQoKTGVuZ3RoVW5pdBIbChdMRU5HVEhfVU5JVF9VTlNQRUNJRklFRBAAEhIKDkxFTkdUSF9VTklUX01NEAESEgoOTEVOR1RIX1VOSVRfQ00QAhIRCg1MRU5HVEhfVU5JVF9NEAMSEgoOTEVOR1RIX1VOSVRfSU4QBBISCg5MRU5HVEhfVU5JVF9GVBAFYgZwcm90bzM");var zr=on("ChljYWRwb2xlL3YxL2RvY3VtZW50LnByb3RvEgpjYWRwb2xlLnYxIvgDCghEb2N1bWVudBIKCgJpZBgBIAEoCRIMCgRuYW1lGAIgASgJEhYKDnNjaGVtYV92ZXJzaW9uGAMgASgNEiwKDGRpc3BsYXlfdW5pdBgEIAEoDjIWLmNhZHBvbGUudjEuTGVuZ3RoVW5pdBInCgdzY3JpcHRzGAUgAygLMhYuY2FkcG9sZS52MS5TY3JpcHRGaWxlEkEKD3BhcmFtX292ZXJyaWRlcxgGIAMoCzIoLmNhZHBvbGUudjEuRG9jdW1lbnQuUGFyYW1PdmVycmlkZXNFbnRyeRIoCglzZWxlY3Rpb24YByABKAsyFS5jYWRwb2xlLnYxLlNlbGVjdGlvbhIjCgR2aWV3GAggASgLMhUuY2FkcG9sZS52MS5WaWV3U3RhdGUSLgoKY3JlYXRlZF9hdBgJIAEoCzIaLmdvb2dsZS5wcm90b2J1Zi5UaW1lc3RhbXASLwoLbW9kaWZpZWRfYXQYCiABKAsyGi5nb29nbGUucHJvdG9idWYuVGltZXN0YW1wEiEKBmFzc2V0cxgLIAMoCzIRLmNhZHBvbGUudjEuQXNzZXQaTQoTUGFyYW1PdmVycmlkZXNFbnRyeRILCgNrZXkYASABKAkSJQoFdmFsdWUYAiABKAsyFi5jYWRwb2xlLnYxLlBhcmFtVmFsdWU6AjgBIrcBCgVBc3NldBIMCgRuYW1lGAEgASgJEg4KBnNoYTI1NhgCIAEoCRIMCgRtaW1lGAMgASgJEgwKBHNpemUYBCABKAQSIwoEa2luZBgFIAEoDjIVLmNhZHBvbGUudjEuQXNzZXRLaW5kEgwKBGRhdGEYBiABKAwSEwoLc291cmNlX3BhdGgYByABKAkSLAoIYWRkZWRfYXQYCCABKAsyGi5nb29nbGUucHJvdG9idWYuVGltZXN0YW1wIioKClNjcmlwdEZpbGUSDAoEcGF0aBgBIAEoCRIOCgZzb3VyY2UYAiABKAkibgoKUGFyYW1WYWx1ZRIQCgZudW1iZXIYASABKAFIABIRCgdib29sZWFuGAIgASgISAASDgoEdGV4dBgDIAEoCUgAEiIKBnZlY3RvchgEIAEoCzIQLmNhZHBvbGUudjEuVmVjM0gAQgcKBXZhbHVlIp0BCglFbnRpdHlSZWYSEgoKZmVhdHVyZV9pZBgBIAEoCRIPCgdib2R5X2lkGAIgASgJEiQKBGtpbmQYAyABKA4yFi5jYWRwb2xlLnYxLkVudGl0eUtpbmQSEQoJdG9wb19uYW1lGAQgASgJEjIKC2ZpbmdlcnByaW50GAUgASgLMh0uY2FkcG9sZS52MS5FbnRpdHlGaW5nZXJwcmludCKNAQoRRW50aXR5RmluZ2VycHJpbnQSFAoMc3VyZmFjZV9raW5kGAEgASgJEiIKCGNlbnRyb2lkGAIgASgLMhAuY2FkcG9sZS52MS5WZWMzEiAKBm5vcm1hbBgDIAEoCzIQLmNhZHBvbGUudjEuVmVjMxIMCgRhcmVhGAQgASgBEg4KBmxlbmd0aBgFIAEoASJqCglTZWxlY3Rpb24SJwoIc2VsZWN0ZWQYASADKAsyFS5jYWRwb2xlLnYxLkVudGl0eVJlZhImCgdob3ZlcmVkGAIgASgLMhUuY2FkcG9sZS52MS5FbnRpdHlSZWYSDAoEbm90ZRgDIAEoCSKuAQoJVmlld1N0YXRlEh0KA2V5ZRgBIAEoCzIQLmNhZHBvbGUudjEuVmVjMxIgCgZ0YXJnZXQYAiABKAsyEC5jYWRwb2xlLnYxLlZlYzMSHAoCdXAYAyABKAsyEC5jYWRwb2xlLnYxLlZlYzMSFAoMb3J0aG9ncmFwaGljGAQgASgIEhMKC2Zvdl9kZWdyZWVzGAUgASgBEhcKD2hpZGRlbl9mZWF0dXJlcxgGIAMoCSrkAQoJQXNzZXRLaW5kEhoKFkFTU0VUX0tJTkRfVU5TUEVDSUZJRUQQABISCg5BU1NFVF9LSU5EX1NWRxABEhIKDkFTU0VUX0tJTkRfRFhGEAISEgoOQVNTRVRfS0lORF9TVEwQAxISCg5BU1NFVF9LSU5EX09CShAEEhYKEkFTU0VUX0tJTkRfVEhSRUVNRhAFEhMKD0FTU0VUX0tJTkRfU1RFUBAGEhQKEEFTU0VUX0tJTkRfSU1BR0UQBxITCg9BU1NFVF9LSU5EX0ZPTlQQCBITCg9BU1NFVF9LSU5EX0dMVEYQCSqBAgoKRW50aXR5S2luZBIbChdFTlRJVFlfS0lORF9VTlNQRUNJRklFRBAAEhcKE0VOVElUWV9LSU5EX0ZFQVRVUkUQARIUChBFTlRJVFlfS0lORF9CT0RZEAISFAoQRU5USVRZX0tJTkRfRkFDRRADEhQKEEVOVElUWV9LSU5EX0VER0UQBBIWChJFTlRJVFlfS0lORF9WRVJURVgQBRIWChJFTlRJVFlfS0lORF9TS0VUQ0gQBhIdChlFTlRJVFlfS0lORF9TS0VUQ0hfRU5USVRZEAcSFQoRRU5USVRZX0tJTkRfUEFSQU0QCBIVChFFTlRJVFlfS0lORF9BU1NFVBAJYgZwcm90bzM",[Qi,Og]);var Uc=on("ChVjYWRwb2xlL3YxL2V2YWwucHJvdG8SCmNhZHBvbGUudjEirgQKBU1vZGVsEiUKBnBhcmFtcxgBIAMoCzIVLmNhZHBvbGUudjEuUGFyYW1JbmZvEiUKCGZlYXR1cmVzGAIgAygLMhMuY2FkcG9sZS52MS5GZWF0dXJlEiAKBmJvZGllcxgDIAMoCzIQLmNhZHBvbGUudjEuQm9keRIrCgtkaWFnbm9zdGljcxgEIAMoCzIWLmNhZHBvbGUudjEuRGlhZ25vc3RpYxITCgtldmFsX21pbGxpcxgFIAEoARIkCgRtZW1vGAYgASgLMhYuY2FkcG9sZS52MS5NZW1vUmVwb3J0Eh8KBXBhcnRzGAcgAygLMhAuY2FkcG9sZS52MS5QYXJ0EicKCWluc3RhbmNlcxgIIAMoCzIULmNhZHBvbGUudjEuSW5zdGFuY2USKAoKYXNzZW1ibGllcxgJIAMoCzIULmNhZHBvbGUudjEuQXNzZW1ibHkSLQoKYW5pbWF0aW9ucxgKIAMoCzIZLmNhZHBvbGUudjEuQW5pbWF0aW9uSW5mbxIhCgZwbGF0ZXMYCyADKAsyES5jYWRwb2xlLnYxLlBsYXRlEjMKD3JlbmRlcl9kZWZhdWx0cxgMIAEoCzIaLmNhZHBvbGUudjEuUmVuZGVyRGVmYXVsdHMSIQoGbGF5ZXJzGBQgAygLMhEuY2FkcG9sZS52MS5MYXllchIvCg1kaXNwbGF5X3VuaXRzGBUgASgLMhguY2FkcG9sZS52MS5EaXNwbGF5VW5pdHMivQEKDlJlbmRlckRlZmF1bHRzEiAKA3N1bhgBIAEoCzITLmNhZHBvbGUudjEuU3VuSW5mbxIVCg1ub3J0aF9kZWdyZWVzGAIgASgBEh0KA2V5ZRgDIAEoCzIQLmNhZHBvbGUudjEuVmVjMxIgCgZ0YXJnZXQYBCABKAsyEC5jYWRwb2xlLnYxLlZlYzMSHAoCdXAYBSABKAsyEC5jYWRwb2xlLnYxLlZlYzMSEwoLZm92X2RlZ3JlZXMYBiABKAEiPAoHU3VuSW5mbxIPCgdhemltdXRoGAEgASgBEhEKCWVsZXZhdGlvbhgCIAEoARINCgVsYWJlbBgDIAEoCSKEAQoFTGF5ZXISDAoEbmFtZRgBIAEoCRILCgNkb2MYAiABKAkSDwoHdmlzaWJsZRgDIAEoCBIPCgdvcGFjaXR5GAQgASgBEiUKBWNvbG9yGAUgASgLMhEuY2FkcG9sZS52MS5Db2xvckgAiAEBEg0KBW9yZGVyGAYgASgNQggKBl9jb2xvciIuCgxEaXNwbGF5VW5pdHMSDAoEdW5pdBgBIAEoCRIQCghmcmFjdGlvbhgCIAEoDSKZAQoFUGxhdGUSDQoFaW5kZXgYASABKA0SDAoEbmFtZRgCIAEoCRIMCgRwYXRoGAMgASgJEg0KBWJlZF94GAQgASgBEg0KBWJlZF95GAUgASgBEiAKBm9yaWdpbhgGIAEoCzIQLmNhZHBvbGUudjEuVmVjMxIlCgZib2RpZXMYByADKAsyFS5jYWRwb2xlLnYxLlBsYXRlQm9keSKYAQoJUGxhdGVCb2R5Eg8KB2JvZHlfaWQYASABKAkSDgoGb2JqZWN0GAIgASgJEigKCXRyYW5zZm9ybRgDIAEoCzIVLmNhZHBvbGUudjEuVHJhbnNmb3JtEgwKBHNsb3QYBCABKA0SIAoFY29sb3IYBSABKAsyES5jYWRwb2xlLnYxLkNvbG9yEhAKCGZpbGFtZW50GAYgASgJIswBCg1BbmltYXRpb25JbmZvEgwKBG5hbWUYASABKAkSEgoKZHVyYXRpb25fcxgCIAEoARILCgNmcHMYAyABKAESDgoGZnJhbWVzGAQgASgNEgwKBGxvb3AYBSABKAgSDAoEZWFzZRgGIAEoCRIOCgZwYXJhbXMYByADKAkSFgoOa2V5ZnJhbWVfdGltZXMYCCADKAESEgoKaGFzX2NhbWVyYRgJIAEoCBIkCgRzcGFuGAogASgLMhYuY2FkcG9sZS52MS5Tb3VyY2VTcGFuIooBCgpNZW1vUmVwb3J0Eg8KB29wc19ydW4YASABKA0SDwoHb3BzX2hpdBgCIAEoDRIUCgxvcHNfZGVmZXJyZWQYAyABKA0SDwoHZW50cmllcxgEIAEoDRINCgVieXRlcxgFIAEoBBITCgtsaW1pdF9ieXRlcxgGIAEoBBIPCgdwcmV2aWV3GAcgASgIIkYKBFBhcnQSCgoCaWQYASABKAkSDAoEbmFtZRgCIAEoCRISCgpmZWF0dXJlX2lkGAMgASgJEhAKCGJvZHlfaWRzGAQgAygJIrIBCghJbnN0YW5jZRIKCgJpZBgBIAEoCRIMCgRuYW1lGAIgASgJEhIKCmZlYXR1cmVfaWQYAyABKAkSDAoEcGFydBgEIAEoCRIjCgRwb3NlGAUgASgLMhUuY2FkcG9sZS52MS5UcmFuc2Zvcm0SIQoHZXhwbG9kZRgGIAEoCzIQLmNhZHBvbGUudjEuVmVjMxIQCghib2R5X2lkcxgHIAMoCRIQCghhc3NlbWJseRgIIAEoCSKQAQoIQXNzZW1ibHkSCgoCaWQYASABKAkSDAoEbmFtZRgCIAEoCRISCgpmZWF0dXJlX2lkGAMgASgJEhEKCWluc3RhbmNlcxgEIAMoCRISCgphc3NlbWJsaWVzGAUgAygJEhgKEGV4cGxvZGVfZGlzdGFuY2UYBiABKAESFQoNZXhwbG9kZV9hbG9uZxgHIAEoCSKtAgoJUGFyYW1JbmZvEgwKBG5hbWUYASABKAkSLQoNZGVmYXVsdF92YWx1ZRgCIAEoCzIWLmNhZHBvbGUudjEuUGFyYW1WYWx1ZRIlCgV2YWx1ZRgDIAEoCzIWLmNhZHBvbGUudjEuUGFyYW1WYWx1ZRISCgpvdmVycmlkZGVuGAQgASgIEhMKC2Rlc2NyaXB0aW9uGAUgASgJEgwKBHVuaXQYBiABKAkSEAoDbWluGAcgASgBSACIAQESEAoDbWF4GAggASgBSAGIAQESEQoEc3RlcBgJIAEoAUgCiAEBEiQKBHNwYW4YCiABKAsyFi5jYWRwb2xlLnYxLlNvdXJjZVNwYW4SDwoHY2hvaWNlcxgLIAMoCUIGCgRfbWluQgYKBF9tYXhCBwoFX3N0ZXAiZAoKU291cmNlU3BhbhIMCgRwYXRoGAEgASgJEhIKCnN0YXJ0X2xpbmUYAiABKA0SEQoJc3RhcnRfY29sGAMgASgNEhAKCGVuZF9saW5lGAQgASgNEg8KB2VuZF9jb2wYBSABKA0iowIKB0ZlYXR1cmUSCgoCaWQYASABKAkSDAoEa2luZBgCIAEoCRIMCgRuYW1lGAMgASgJEg4KBmlucHV0cxgEIAMoCRIPCgdvdXRwdXRzGAUgAygJEisKBGFyZ3MYBiADKAsyHS5jYWRwb2xlLnYxLkZlYXR1cmUuQXJnc0VudHJ5EiQKBHNwYW4YByABKAsyFi5jYWRwb2xlLnYxLlNvdXJjZVNwYW4SEgoKc3VwcHJlc3NlZBgIIAEoCBIrCgtkaWFnbm9zdGljcxgJIAMoCzIWLmNhZHBvbGUudjEuRGlhZ25vc3RpYxIOCgZjYWNoZWQYCiABKAgaKwoJQXJnc0VudHJ5EgsKA2tleRgBIAEoCRINCgV2YWx1ZRgCIAEoCToCOAEiswIKBEJvZHkSCgoCaWQYASABKAkSEgoKZmVhdHVyZV9pZBgCIAEoCRIMCgRuYW1lGAMgASgJEh4KBGJib3gYBCABKAsyEC5jYWRwb2xlLnYxLkJCb3gSJgoIdG9wb2xvZ3kYBSABKAsyFC5jYWRwb2xlLnYxLlRvcG9sb2d5Eh4KBG1lc2gYBiABKAsyEC5jYWRwb2xlLnYxLk1lc2gSDwoHdmlzaWJsZRgHIAEoCBImCghtYXRlcmlhbBgIIAEoCzIULmNhZHBvbGUudjEuTWF0ZXJpYWwSEQoJcGFydF9ib2R5GAkgASgJEigKCXRyYW5zZm9ybRgKIAEoCzIVLmNhZHBvbGUudjEuVHJhbnNmb3JtEhAKCGluc3RhbmNlGAsgASgJEg0KBWxheWVyGBQgASgJIigKBUNvbG9yEgkKAXIYASABKAESCQoBZxgCIAEoARIJCgFiGAMgASgBIq0DCghNYXRlcmlhbBIMCgRuYW1lGAEgASgJEiUKCmJhc2VfY29sb3IYAiABKAsyES5jYWRwb2xlLnYxLkNvbG9yEhEKCXJvdWdobmVzcxgDIAEoARIQCghtZXRhbGxpYxgEIAEoARIPCgd0ZXh0dXJlGAUgASgJEhUKDXRleHR1cmVfc2NhbGUYBiABKAESFAoHb3BhY2l0eRgHIAEoAUgAiAEBEhAKA2lvchgIIAEoAUgBiAEBEiMKCGVtaXNzaXZlGAkgASgLMhEuY2FkcG9sZS52MS5Db2xvchIUCgx0cmFuc21pc3Npb24YCiABKAESHAoUYXR0ZW51YXRpb25fZGlzdGFuY2UYCyABKAESGAoQc2NhdHRlcl9kaXN0YW5jZRgMIAEoARIRCglzY2F0dGVyX2cYDSABKAESEgoKYW5pc290cm9weRgOIAEoARISCgplZGdlX2JyZWFrGA8gASgBEhUKDXRleHR1cmVfZGVwdGgYECABKAESEwoGc2hhZG93GBQgASgISAKIAQFCCgoIX29wYWNpdHlCBgoEX2lvckIJCgdfc2hhZG93In4KCFRvcG9sb2d5EiMKBWZhY2VzGAEgAygLMhQuY2FkcG9sZS52MS5Ub3BvRmFjZRIjCgVlZGdlcxgCIAMoCzIULmNhZHBvbGUudjEuVG9wb0VkZ2USKAoIdmVydGljZXMYAyADKAsyFi5jYWRwb2xlLnYxLlRvcG9WZXJ0ZXgi1wEKCFRvcG9GYWNlEgwKBG5hbWUYASABKAkSFAoMc3VyZmFjZV9raW5kGAIgASgJEiIKCGNlbnRyb2lkGAMgASgLMhAuY2FkcG9sZS52MS5WZWMzEiAKBm5vcm1hbBgEIAEoCzIQLmNhZHBvbGUudjEuVmVjMxIMCgRhcmVhGAUgASgBEg0KBWVkZ2VzGAYgAygJEhEKCXRyaV9zdGFydBgHIAEoDRIRCgl0cmlfY291bnQYCCABKA0SDQoFaW5kZXgYCSABKA0SDwoHYWxpYXNlcxgKIAMoCSLoAQoIVG9wb0VkZ2USDAoEbmFtZRgBIAEoCRISCgpjdXJ2ZV9raW5kGAIgASgJEh8KBXN0YXJ0GAMgASgLMhAuY2FkcG9sZS52MS5WZWMzEh0KA2VuZBgEIAEoCzIQLmNhZHBvbGUudjEuVmVjMxIOCgZsZW5ndGgYBSABKAESDQoFZmFjZXMYBiADKAkSIgoIcG9seWxpbmUYByADKAsyEC5jYWRwb2xlLnYxLlZlYzMSGAoQZGloZWRyYWxfZGVncmVlcxgIIAEoARIOCgZjb252ZXgYCSABKAgSDQoFaW5kZXgYCiABKA0iXAoKVG9wb1ZlcnRleBIMCgRuYW1lGAEgASgJEiIKCHBvc2l0aW9uGAIgASgLMhAuY2FkcG9sZS52MS5WZWMzEg0KBWluZGV4GAMgASgNEg0KBWZhY2VzGAQgAygJIm0KBE1lc2gSFQoJcG9zaXRpb25zGAEgAygCQgIQARITCgdub3JtYWxzGAIgAygCQgIQARITCgdpbmRpY2VzGAMgAygNQgIQARIUCghmYWNlX2lkcxgEIAMoDUICEAESDgoGY29sb3JzGAUgASgMIn8KCkRpYWdub3N0aWMSJgoIc2V2ZXJpdHkYASABKA4yFC5jYWRwb2xlLnYxLlNldmVyaXR5Eg8KB21lc3NhZ2UYAiABKAkSJAoEc3BhbhgDIAEoCzIWLmNhZHBvbGUudjEuU291cmNlU3BhbhISCgpmZWF0dXJlX2lkGAQgASgJIvYCCgpCYWtlZE1vZGVsEhYKDnNjaGVtYV92ZXJzaW9uGAEgASgNEgwKBG5hbWUYAiABKAkSIAoFbW9kZWwYAyABKAsyES5jYWRwb2xlLnYxLk1vZGVsEhEKCXRvbGVyYW5jZRgEIAEoARIRCglnZW5lcmF0b3IYBSABKAkSFQoNc291cmNlX3NoYTI1NhgGIAEoCRJDCg9wYXJhbV9vdmVycmlkZXMYByADKAsyKi5jYWRwb2xlLnYxLkJha2VkTW9kZWwuUGFyYW1PdmVycmlkZXNFbnRyeRImCghkb2N1bWVudBgIIAEoCzIULmNhZHBvbGUudjEuRG9jdW1lbnQSJwoEdmlldxgJIAEoCzIZLmNhZHBvbGUudjEuUHVibGlzaGVkVmlldxpNChNQYXJhbU92ZXJyaWRlc0VudHJ5EgsKA2tleRgBIAEoCRIlCgV2YWx1ZRgCIAEoCzIWLmNhZHBvbGUudjEuUGFyYW1WYWx1ZToCOAEiyQEKDVB1Ymxpc2hlZFZpZXcSDgoGY2FtZXJhGAEgASgJEh0KA2V5ZRgCIAEoCzIQLmNhZHBvbGUudjEuVmVjMxIgCgZ0YXJnZXQYAyABKAsyEC5jYWRwb2xlLnYxLlZlYzMSDAoEZ3JpZBgEIAEoCRIPCgdzZWN0aW9uGAUgASgJEhQKDGZyYW1lX2xheWVycxgGIAMoCRIMCgRtb2RlGAcgASgJEhAKCGhvbWVfdXJsGAggASgJEhIKCmhvbWVfbGFiZWwYCSABKAkqYQoIU2V2ZXJpdHkSGAoUU0VWRVJJVFlfVU5TUEVDSUZJRUQQABIRCg1TRVZFUklUWV9JTkZPEAESFAoQU0VWRVJJVFlfV0FSTklORxACEhIKDlNFVkVSSVRZX0VSUk9SEANiBnByb3RvMw",[zr,Qi]);var Dc=on("ChhjYWRwb2xlL3YxL29wdGlvbnMucHJvdG8SB2NhZHBvbGUifAoJT3BlcmF0aW9uEhAKCGNsaV9uYW1lGAEgASgJEhIKCmlkZW1wb3RlbnQYAiABKAgSFQoNbmVlZHNfc2Vzc2lvbhgDIAEoCBITCgtpbWFnZV9maWVsZBgEIAEoCRINCgVhbGlhcxgFIAMoCRIOCgZoaWRkZW4YBiABKAgiawoFRmllbGQSDAoEZmxhZxgBIAEoCRISCgpwb3NpdGlvbmFsGAIgASgIEg8KB2V4YW1wbGUYAyABKAkSDgoGaGlkZGVuGAQgASgIEg0KBXNob3J0GAUgASgJEhAKCHJlcXVpcmVkGAYgASgIIiUKBFR5cGUSDQoFdGl0bGUYASABKAkSDgoGaGlkZGVuGAIgASgIOkQKAm9wEh4uZ29vZ2xlLnByb3RvYnVmLk1ldGhvZE9wdGlvbnMYoZYDIAEoCzISLmNhZHBvbGUuT3BlcmF0aW9uUgJvcDpFCgVmaWVsZBIdLmdvb2dsZS5wcm90b2J1Zi5GaWVsZE9wdGlvbnMYoZYDIAEoCzIOLmNhZHBvbGUuRmllbGRSBWZpZWxkOkQKBHR5cGUSHy5nb29nbGUucHJvdG9idWYuTWVzc2FnZU9wdGlvbnMYoZYDIAEoCzINLmNhZHBvbGUuVHlwZVIEdHlwZWIGcHJvdG8z",[pd]);var Xg=on("ChdjYWRwb2xlL3YxL2VuZ2luZS5wcm90bxIKY2FkcG9sZS52MSL6AQoHUmVxdWVzdBIvCghldmFsdWF0ZRgBIAEoCzIbLmNhZHBvbGUudjEuRXZhbHVhdGVSZXF1ZXN0SAASKwoGZXhwb3J0GAIgASgLMhkuY2FkcG9sZS52MS5FeHBvcnRSZXF1ZXN0SAASLAoHYXBpX2RvYxgDIAEoCzIZLmNhZHBvbGUudjEuQXBpRG9jUmVxdWVzdEgAEisKBmZvcm1hdBgEIAEoCzIZLmNhZHBvbGUudjEuRm9ybWF0UmVxdWVzdEgAEisKBnJlbmRlchgFIAEoCzIZLmNhZHBvbGUudjEuUmVuZGVyUmVxdWVzdEgAQgkKB3JlcXVlc3QikgIKCFJlc3BvbnNlEjAKCGV2YWx1YXRlGAEgASgLMhwuY2FkcG9sZS52MS5FdmFsdWF0ZVJlc3BvbnNlSAASLAoGZXhwb3J0GAIgASgLMhouY2FkcG9sZS52MS5FeHBvcnRSZXNwb25zZUgAEi0KB2FwaV9kb2MYAyABKAsyGi5jYWRwb2xlLnYxLkFwaURvY1Jlc3BvbnNlSAASLAoGZm9ybWF0GAQgASgLMhouY2FkcG9sZS52MS5Gb3JtYXRSZXNwb25zZUgAEiwKBnJlbmRlchgFIAEoCzIaLmNhZHBvbGUudjEuUmVuZGVyUmVzcG9uc2VIABIPCgVlcnJvchgPIAEoCUgAQgoKCHJlc3BvbnNlIrEDCg9FdmFsdWF0ZVJlcXVlc3QSLgoIZG9jdW1lbnQYASABKAsyFC5jYWRwb2xlLnYxLkRvY3VtZW50QgaKshkCIAESHQoJdG9sZXJhbmNlGAIgASgBQgqKshkGGgQwLjA1EhEKCXNraXBfbWVzaBgDIAEoCBIlChFib29sZWFuX3RvbGVyYW5jZRgEIAEoAUIKirIZBhoEMC4wNRJGCgphc3NldF9kYXRhGAUgAygLMiouY2FkcG9sZS52MS5FdmFsdWF0ZVJlcXVlc3QuQXNzZXREYXRhRW50cnlCBoqyGQIgARIgCgtkZWFkbGluZV9tcxgGIAEoBEILirIZBxoFMjAwMDASDwoHcHJldmlldxgHIAEoCBIWCg5tYXhfaGVhcF9ieXRlcxgeIAEoBBIRCgltYXhfdGlja3MYHyABKAQSFQoNbWF4X2NhbGxzdGFjaxggIAEoDRowCg5Bc3NldERhdGFFbnRyeRILCgNrZXkYASABKAkSDQoFdmFsdWUYAiABKAw6AjgBOiaKshkiCiBFdmFsdWF0ZSBhIGRvY3VtZW50IGludG8gYSBtb2RlbCI0ChBFdmFsdWF0ZVJlc3BvbnNlEiAKBW1vZGVsGAEgASgLMhEuY2FkcG9sZS52MS5Nb2RlbCKqBgoNRXhwb3J0UmVxdWVzdBIuCghkb2N1bWVudBgBIAEoCzIULmNhZHBvbGUudjEuRG9jdW1lbnRCBoqyGQIgARJBCgZmb3JtYXQYAiABKA4yGC5jYWRwb2xlLnYxLkV4cG9ydEZvcm1hdEIXirIZExoRRVhQT1JUX0ZPUk1BVF9TVEwSEAoIYm9keV9pZHMYAyADKAkSEQoJdG9sZXJhbmNlGAQgASgBEhoKEnJ1bl9zY3JpcHRfZXhwb3J0cxgFIAEoCBIZChFib29sZWFuX3RvbGVyYW5jZRgGIAEoARIbCgZzbGljZXIYByABKAlCC4qyGQcaBWJhbWJ1EhkKBWdyb3VwGAggASgJQgqKshkGGgRwYXJ0EhsKBmZsYXZvchgJIAEoCUILirIZBxoFYmFtYnUSDQoFcGxhdGUYCiABKA0SIAoHcHJpbnRlchgLIAEoCUIPirIZCxoJYmFtYnVfaDJkEhQKDHBsYXRlX2ltYWdlcxgMIAEoCBIlChFwbGF0ZV9pbWFnZV93aWR0aBgNIAEoDUIKirIZBhoEMTYwMBImChJwbGF0ZV9pbWFnZV9oZWlnaHQYDiABKA1CCoqyGQYaBDEyMDASLAoWcGxhdGVfaW1hZ2VfYmFja2dyb3VuZBgPIAEoCUIMirIZCBoGc3R1ZGlvEjYKE3BsYXRlX2ltYWdlX2JhY2tlbmQYECABKA4yGS5jYWRwb2xlLnYxLlJlbmRlckJhY2tlbmQSGwoTc2NyaXB0X2V4cG9ydHNfb25seRgRIAEoCBIdChVpbmNsdWRlX2hpZGRlbl9sYXllcnMYEiABKAgSHAoKbW9kZWxfdW5pdBgTIAEoCUIIirIZBBoCZnQSGgoFdW5pdHMYFCABKAlCC4qyGQcaBWZ0LWluEikKCHRlbXBsYXRlGBUgASgJQheKshkTGhFoMmRfNF9jb2xvdXJzLjNtZhILCgNiZWQYFiADKAESJwoLbGF5ZXJfbmFtZXMYFyADKAlCEoqyGQ4KBWxheWVyGgV3YWxsczoiirIZHgocRXhwb3J0IGJvZGllcyB0byBhIG1lc2ggZmlsZSL/AQoORXhwb3J0UmVzcG9uc2USDAoEZGF0YRgBIAEoDBIaChJzdWdnZXN0ZWRfZmlsZW5hbWUYAiABKAkSJQoFZmlsZXMYAyADKAsyFi5jYWRwb2xlLnYxLkV4cG9ydEZpbGUSKwoLZGlhZ25vc3RpY3MYBCADKAsyFi5jYWRwb2xlLnYxLkRpYWdub3N0aWMSLAoGYm9kaWVzGAUgAygLMhwuY2FkcG9sZS52MS5FeHBvcnRCb2R5UmVwb3J0EiwKDHBsYXRlX2ltYWdlcxgGIAMoCzIWLmNhZHBvbGUudjEuUGxhdGVJbWFnZRITCgtwbGF0ZV9zaGVldBgHIAEoDCKZAQoKRXhwb3J0RmlsZRIMCgRwYXRoGAEgASgJEgwKBGRhdGEYAiABKAwSLAoGYm9kaWVzGAMgAygLMhwuY2FkcG9sZS52MS5FeHBvcnRCb2R5UmVwb3J0EiwKDHBsYXRlX2ltYWdlcxgEIAMoCzIWLmNhZHBvbGUudjEuUGxhdGVJbWFnZRITCgtwbGF0ZV9zaGVldBgFIAEoDCJpCgpQbGF0ZUltYWdlEgwKBHBhdGgYASABKAkSDQoFcGxhdGUYAiABKA0SEgoKcGxhdGVfbmFtZRgDIAEoCRINCgV3aWR0aBgEIAEoDRIOCgZoZWlnaHQYBSABKA0SCwoDcG5nGAYgASgMIoYCChBFeHBvcnRCb2R5UmVwb3J0Eg8KB2JvZHlfaWQYASABKAkSDAoEbmFtZRgCIAEoCRIRCgl0cmlhbmdsZXMYAyABKA0SEQoJdG9sZXJhbmNlGAQgASgBEhgKEHRvbGVyYW5jZV9zb3VyY2UYBSABKAkSFgoOdGVzc2VsbGF0ZWRfYXQYBiABKAESGAoLY2hvcmRfZXJyb3IYByABKAFIAIgBARIYChBjaG9yZF9lcnJvcl9mYWNlGAggASgJEg0KBWV4YWN0GAkgASgIEigKDmNob3JkX2Vycm9yX2F0GAogASgLMhAuY2FkcG9sZS52MS5WZWMzQg4KDF9jaG9yZF9lcnJvciIPCg1BcGlEb2NSZXF1ZXN0ImAKDkFwaURvY1Jlc3BvbnNlEioKCWZ1bmN0aW9ucxgBIAMoCzIXLmNhZHBvbGUudjEuQXBpRnVuY3Rpb24SIgoFdHlwZXMYAiADKAsyEy5jYWRwb2xlLnYxLkFwaVR5cGUiggEKC0FwaUZ1bmN0aW9uEgwKBG5hbWUYASABKAkSCwoDZG9jGAIgASgJEiQKBnBhcmFtcxgDIAMoCzIULmNhZHBvbGUudjEuQXBpUGFyYW0SDwoHcmV0dXJucxgEIAEoCRIPCgdvbl90eXBlGAUgASgJEhAKCGNvbnN0YW50GAYgASgIIlwKCEFwaVBhcmFtEgwKBG5hbWUYASABKAkSDAoEdHlwZRgCIAEoCRILCgNkb2MYAyABKAkSEAoIcmVxdWlyZWQYBCABKAgSFQoNZGVmYXVsdF92YWx1ZRgFIAEoCSJzCgdBcGlUeXBlEgwKBG5hbWUYASABKAkSCwoDZG9jGAIgASgJEigKB21ldGhvZHMYAyADKAsyFy5jYWRwb2xlLnYxLkFwaUZ1bmN0aW9uEiMKBWF0dHJzGAQgAygLMhQuY2FkcG9sZS52MS5BcGlQYXJhbSIfCg1Gb3JtYXRSZXF1ZXN0Eg4KBnNvdXJjZRgBIAEoCSIgCg5Gb3JtYXRSZXNwb25zZRIOCgZzb3VyY2UYASABKAkiXgoKTGF5ZXJTdGF0ZRIMCgRuYW1lGAEgASgJEhQKB3Zpc2libGUYAiABKAhIAIgBARIUCgdvcGFjaXR5GAMgASgBSAGIAQFCCgoIX3Zpc2libGVCCgoIX29wYWNpdHkixwEKBkNhbWVyYRIfCgZwcmVzZXQYASABKAlCD4qyGQsKBHZpZXcaA2lzbxIdCgNleWUYAiABKAsyEC5jYWRwb2xlLnYxLlZlYzMSIAoGdGFyZ2V0GAMgASgLMhAuY2FkcG9sZS52MS5WZWMzEhwKAnVwGAQgASgLMhAuY2FkcG9sZS52MS5WZWMzEhQKDG9ydGhvZ3JhcGhpYxgFIAEoCBITCgtmb3ZfZGVncmVlcxgGIAEoARISCgpmaXRfbWFyZ2luGAcgASgBIpwMCg1SZW5kZXJSZXF1ZXN0Ei4KCGRvY3VtZW50GAEgASgLMhQuY2FkcG9sZS52MS5Eb2N1bWVudEIGirIZAiABEhgKBXdpZHRoGAIgASgNQgmKshkFGgM4MDASGQoGaGVpZ2h0GAMgASgNQgmKshkFGgM2MDASIgoGY2FtZXJhGAQgASgLMhIuY2FkcG9sZS52MS5DYW1lcmESKgoHYmFja2VuZBgFIAEoDjIZLmNhZHBvbGUudjEuUmVuZGVyQmFja2VuZBIPCgdzYW1wbGVzGAYgASgNEiUKCmJhY2tncm91bmQYByABKAsyES5jYWRwb2xlLnYxLkNvbG9yEhIKCmRyYXdfZWRnZXMYCCABKAgSEQoJZHJhd19ncmlkGAkgASgIEhEKCXRvbGVyYW5jZRgKIAEoARIjCghib2R5X2lkcxgLIAMoCUIRirIZDRoLZXh0cnVkZV8xIzASXQoPcGFyYW1fb3ZlcnJpZGVzGAwgAygLMi0uY2FkcG9sZS52MS5SZW5kZXJSZXF1ZXN0LlBhcmFtT3ZlcnJpZGVzRW50cnlCFYqyGREKBXBhcmFtGgh3YWxsPTIuNBIbChNoaWdobGlnaHRfc2VsZWN0aW9uGA0gASgIEg8KB3RocmVhZHMYDiABKA0SDQoFZHJhZnQYDyABKAgSGwoTZWRnZV9jcmVhc2VfZGVncmVlcxgQIAEoARInCgllZGdlX21vZGUYESABKA4yFC5jYWRwb2xlLnYxLkVkZ2VNb2RlEicKCWdyaWRfbW9kZRgSIAEoDjIULmNhZHBvbGUudjEuR3JpZE1vZGUSDAoEeHJheRgTIAEoCBIcChRub19hbWJpZW50X29jY2x1c2lvbhgUIAEoCBIZChFub19jb250YWN0X3NoYWRvdxgVIAEoCBIWCg5ub19zaWxob3VldHRlcxgWIAEoCBIQCghleHBvc3VyZRgXIAEoARIQCghub19lZGdlcxgYIAEoCBIPCgdub19ncmlkGBkgASgIEhUKDWVkZ2Vfd2lkdGhfcHgYGiABKAESGQoRYm9vbGVhbl90b2xlcmFuY2UYGyABKAESEwoLbWF4X2JvdW5jZXMYHCABKA0SIQoIYW5hbHlzaXMYHSABKAlCD4qyGQsaCXRoaWNrbmVzcxIQCghtaW5fd2FsbBgeIAEoARIcChRtYXhfb3ZlcmhhbmdfZGVncmVlcxgfIAEoARIYChBzaG93X2Vudmlyb25tZW50GCAgASgIEhoKBWRlYnVnGCEgASgJQguKshkHGgVmYWNlcxIfCghsaWdodGluZxgiIAEoCUINirIZCRoHcHJvZHVjdBIeCghiYWNrZHJvcBgjIAEoCUIMirIZCBoGc3R1ZGlvEhoKBWZsb29yGCQgASgJQguKshkHGgVnbG9zcxIeCgd0b25lbWFwGCUgASgJQg2KshkJGgduZXV0cmFsEh8KDGRvZl9mX251bWJlchgmIAEoAUIJirIZBQoDZG9mEg0KBWJsb29tGCcgASgIEicKDWVkZ2VfYnJlYWtfbW0YKCABKAFCEIqyGQwKCmVkZ2UtYnJlYWsSDwoHZGVub2lzZRgpIAEoCBIuCgNzdW4YKiABKAlCIYqyGR0aGzM4LC0xMjEsMjAyNi0xMi0yMSwxMjowMCwtOBIiCg1ub3J0aF9kZWdyZWVzGCsgASgBQguKshkHCgVub3J0aBIbCgdzZWN0aW9uGCwgASgJQgqKshkGGgR6PTQwEhMKC3RyYW5zcGFyZW50GC0gASgIEhYKDnNoYWRvd19jYXRjaGVyGC4gASgIEhMKC3N1cGVyc2FtcGxlGC8gASgNEkEKBmxheWVycxgyIAMoCzIWLmNhZHBvbGUudjEuTGF5ZXJTdGF0ZUIZirIZFQoFbGF5ZXIaDGVudmVsb3BlPW9mZhpNChNQYXJhbU92ZXJyaWRlc0VudHJ5EgsKA2tleRgBIAEoCRIlCgV2YWx1ZRgCIAEoCzIWLmNhZHBvbGUudjEuUGFyYW1WYWx1ZToCOAE6JIqyGSAKHlJlbmRlciBvbmUgaW1hZ2Ugb2YgYSBkb2N1bWVudCLjAQoOUmVuZGVyUmVzcG9uc2USCwoDcG5nGAEgASgMEg0KBXdpZHRoGAIgASgNEg4KBmhlaWdodBgDIAEoDRIoCgVtb2RlbBgEIAEoCzIRLmNhZHBvbGUudjEuTW9kZWxCBoqyGQIgARImCgVzdGF0cxgFIAEoCzIXLmNhZHBvbGUudjEuUmVuZGVyU3RhdHMSKwoLZGlhZ25vc3RpY3MYBiADKAsyFi5jYWRwb2xlLnYxLkRpYWdub3N0aWMSJgoGY2FtZXJhGAcgASgLMhYuY2FkcG9sZS52MS5DYW1lcmFJbmZvIpsECgpDYW1lcmFJbmZvEg0KBWluZGV4GAEgASgNEgkKAXQYAiABKAESDQoFd2lkdGgYAyABKA0SDgoGaGVpZ2h0GAQgASgNEh0KA2V5ZRgFIAEoCzIQLmNhZHBvbGUudjEuVmVjMxIgCgZ0YXJnZXQYBiABKAsyEC5jYWRwb2xlLnYxLlZlYzMSHAoCdXAYByABKAsyEC5jYWRwb2xlLnYxLlZlYzMSEwoLZm92X2RlZ3JlZXMYCCABKAESFAoMb3J0aG9ncmFwaGljGAkgASgIEhQKDG9ydGhvX2hlaWdodBgKIAEoARIMCgR2aWV3GAsgAygBEhIKCnByb2plY3Rpb24YDCADKAESDAoEbmVhchgNIAEoARILCgNmYXIYDiABKAESJwoNZ3JvdW5kX29yaWdpbhgPIAEoCzIQLmNhZHBvbGUudjEuVmVjMxInCg1ncm91bmRfbm9ybWFsGBAgASgLMhAuY2FkcG9sZS52MS5WZWMzEiIKCGdyb3VuZF91GBEgASgLMhAuY2FkcG9sZS52MS5WZWMzEiIKCGdyb3VuZF92GBIgASgLMhAuY2FkcG9sZS52MS5WZWMzEiQKCmJvdW5kc19taW4YEyABKAsyEC5jYWRwb2xlLnYxLlZlYzMSJAoKYm91bmRzX21heBgUIAEoCzIQLmNhZHBvbGUudjEuVmVjMxIRCglib3VuZHNfcHgYFSADKAEiyQEKC1JlbmRlclN0YXRzEhAKCHRvdGFsX21zGAEgASgBEg4KBnBhc3NlcxgCIAEoDRIUCgx0aHJlYWRzX3VzZWQYAyABKA0SEwoLZ2VvbWV0cnlfbXMYBCABKAESDwoHc3Nhb19tcxgFIAEoARISCgpjb250YWN0X21zGAYgASgBEhAKCHNoYWRlX21zGAcgASgBEg4KBm9pdF9tcxgIIAEoARISCgpvdmVybGF5X21zGAkgASgBEhIKCnJlc29sdmVfbXMYCiABKAEqggIKDEV4cG9ydEZvcm1hdBIdChlFWFBPUlRfRk9STUFUX1VOU1BFQ0lGSUVEEAASFQoRRVhQT1JUX0ZPUk1BVF9TVEwQARIVChFFWFBPUlRfRk9STUFUX09CShACEhYKEkVYUE9SVF9GT1JNQVRfU1RFUBADEhUKEUVYUE9SVF9GT1JNQVRfR0xCEAQSGQoVRVhQT1JUX0ZPUk1BVF9USFJFRU1GEAUSFQoRRVhQT1JUX0ZPUk1BVF9TVkcQBhIVChFFWFBPUlRfRk9STUFUX1BERhAHEhUKEUVYUE9SVF9GT1JNQVRfRFhGEAgSFgoSRVhQT1JUX0ZPUk1BVF9HTFRGEAkqaQoNUmVuZGVyQmFja2VuZBIeChpSRU5ERVJfQkFDS0VORF9VTlNQRUNJRklFRBAAEhkKFVJFTkRFUl9CQUNLRU5EX1JBU1RFUhABEh0KGVJFTkRFUl9CQUNLRU5EX1BBVEhfVFJBQ0UQAip8CghFZGdlTW9kZRIZChVFREdFX01PREVfVU5TUEVDSUZJRUQQABIRCg1FREdFX01PREVfT0ZGEAESGAoURURHRV9NT0RFX1NJTEhPVUVUVEUQAhIVChFFREdFX01PREVfQ1JFQVNFUxADEhEKDUVER0VfTU9ERV9BTEwQBCpmCghHcmlkTW9kZRIZChVHUklEX01PREVfVU5TUEVDSUZJRUQQABIRCg1HUklEX01PREVfT0ZGEAESFAoQR1JJRF9NT0RFX0ZJTklURRACEhYKEkdSSURfTU9ERV9JTkZJTklURRADYgZwcm90bzM",[zr,Uc,Qi,Dc]);var NE=on("ChRjYWRwb2xlL3YxL29wcy5wcm90bxIKY2FkcG9sZS52MSI1CgtQYXJhbVZhbHVlcxImCgZ2YWx1ZXMYASADKAsyFi5jYWRwb2xlLnYxLlBhcmFtVmFsdWUitAkKDFN3ZWVwUmVxdWVzdBIuCghkb2N1bWVudBgBIAEoCzIULmNhZHBvbGUudjEuRG9jdW1lbnRCBoqyGQIgARJPCgZwYXJhbXMYAiADKAsyJC5jYWRwb2xlLnYxLlN3ZWVwUmVxdWVzdC5QYXJhbXNFbnRyeUIZirIZFQoFcGFyYW0aDHdhbGw9MS42LDIuNBIYCgV3aWR0aBgDIAEoDUIJirIZBRoDMzIwEhkKBmhlaWdodBgEIAEoDUIJirIZBRoDMjQwEiIKBmNhbWVyYRgFIAEoCzISLmNhZHBvbGUudjEuQ2FtZXJhEhUKBGNvbHMYBiABKA1CB4qyGQMaATMSDwoHc2FtcGxlcxgHIAEoDRIPCgd0aHJlYWRzGAggASgNEhEKCXRvbGVyYW5jZRgJIAEoARIRCgltYXhfY2VsbHMYCiABKA0SEAoIbm9fZWRnZXMYDCABKAgSDwoHbm9fZ3JpZBgNIAEoCBIcChRub19hbWJpZW50X29jY2x1c2lvbhgOIAEoCBIZChFub19jb250YWN0X3NoYWRvdxgPIAEoCBIWCg5ub19zaWxob3VldHRlcxgQIAEoCBIQCghleHBvc3VyZRgRIAEoARINCgVkcmFmdBgSIAEoCBITCgtwYXJhbV9vcmRlchgLIAMoCRIZChFib29sZWFuX3RvbGVyYW5jZRgTIAEoARIbChNlZGdlX2NyZWFzZV9kZWdyZWVzGBQgASgBEicKCWVkZ2VfbW9kZRgVIAEoDjIULmNhZHBvbGUudjEuRWRnZU1vZGUSJwoJZ3JpZF9tb2RlGBYgASgOMhQuY2FkcG9sZS52MS5HcmlkTW9kZRIMCgR4cmF5GBcgASgIEhUKDWVkZ2Vfd2lkdGhfcHgYGCABKAESJQoKYmFja2dyb3VuZBgZIAEoCzIRLmNhZHBvbGUudjEuQ29sb3ISGAoQc2hvd19lbnZpcm9ubWVudBgaIAEoCBIfCghsaWdodGluZxgbIAEoCUINirIZCRoHcHJvZHVjdBIeCghiYWNrZHJvcBgcIAEoCUIMirIZCBoGc3R1ZGlvEhoKBWZsb29yGB0gASgJQguKshkHGgVnbG9zcxIeCgd0b25lbWFwGB4gASgJQg2KshkJGgduZXV0cmFsEi4KA3N1bhgfIAEoCUIhirIZHRobMzgsLTEyMSwyMDI2LTEyLTIxLDEyOjAwLC04EiIKDW5vcnRoX2RlZ3JlZXMYICABKAFCC4qyGQcKBW5vcnRoEhMKC3RyYW5zcGFyZW50GCEgASgIEhYKDnNoYWRvd19jYXRjaGVyGCIgASgIEhMKC3N1cGVyc2FtcGxlGCMgASgNEkEKBmxheWVycxgyIAMoCzIWLmNhZHBvbGUudjEuTGF5ZXJTdGF0ZUIZirIZFQoFbGF5ZXIaDGVudmVsb3BlPW9mZhpGCgtQYXJhbXNFbnRyeRILCgNrZXkYASABKAkSJgoFdmFsdWUYAiABKAsyFy5jYWRwb2xlLnYxLlBhcmFtVmFsdWVzOgI4ATozirIZLwotUmVuZGVyIGEgcGFyYW1ldGVyIHN3ZWVwIGFzIG9uZSBjb250YWN0IHNoZWV0IvMLCg5BbmltYXRlUmVxdWVzdBIuCghkb2N1bWVudBgBIAEoCzIULmNhZHBvbGUudjEuRG9jdW1lbnRCBoqyGQIgARIlCgRuYW1lGAIgASgJQheKshkTGhFhc3NlbWJsZV9hbmRfdHVybhIMCgRsaXN0GAMgASgIEhkKBmZvcm1hdBgEIAEoCUIJirIZBRoDZ2lmEhgKBXdpZHRoGAUgASgNQgmKshkFGgM4MDASGQoGaGVpZ2h0GAYgASgNQgmKshkFGgM2MDASIgoGY2FtZXJhGAcgASgLMhIuY2FkcG9sZS52MS5DYW1lcmESKgoHYmFja2VuZBgIIAEoDjIZLmNhZHBvbGUudjEuUmVuZGVyQmFja2VuZBIPCgdzYW1wbGVzGAkgASgNEg8KB3RocmVhZHMYCiABKA0SDQoFZHJhZnQYCyABKAgSCwoDZnBzGAwgASgBEhgKBmZyYW1lcxgNIAEoDUIIirIZBBoCMjQSDwoHcHJldmlldxgOIAEoCBIRCgl0b2xlcmFuY2UYDyABKAESGQoRYm9vbGVhbl90b2xlcmFuY2UYECABKAESaAoPcGFyYW1fb3ZlcnJpZGVzGBEgAygLMi4uY2FkcG9sZS52MS5BbmltYXRlUmVxdWVzdC5QYXJhbU92ZXJyaWRlc0VudHJ5Qh+KshkbCgVwYXJhbRoScHJpbnRfbGF5b3V0PWZhbHNlEhAKCGJvZHlfaWRzGBIgAygJEhAKCG5vX2VkZ2VzGBQgASgIEg8KB25vX2dyaWQYFSABKAgSHAoUbm9fYW1iaWVudF9vY2NsdXNpb24YFiABKAgSGQoRbm9fY29udGFjdF9zaGFkb3cYFyABKAgSFgoObm9fc2lsaG91ZXR0ZXMYGCABKAgSEAoIZXhwb3N1cmUYGSABKAESJQoKYmFja2dyb3VuZBgaIAEoCzIRLmNhZHBvbGUudjEuQ29sb3ISGAoQc2hvd19lbnZpcm9ubWVudBgbIAEoCBIfCghsaWdodGluZxgcIAEoCUINirIZCRoHcHJvZHVjdBIeCghiYWNrZHJvcBgdIAEoCUIMirIZCBoGc3R1ZGlvEhoKBWZsb29yGB4gASgJQguKshkHGgVnbG9zcxIeCgd0b25lbWFwGB8gASgJQg2KshkJGgduZXV0cmFsEh8KDGRvZl9mX251bWJlchggIAEoAUIJirIZBQoDZG9mEg0KBWJsb29tGCEgASgIEicKDWVkZ2VfYnJlYWtfbW0YIiABKAFCEIqyGQwKCmVkZ2UtYnJlYWsSDwoHZGVub2lzZRgjIAEoCBITCgttYXhfYm91bmNlcxgkIAEoDRInCgllZGdlX21vZGUYJSABKA4yFC5jYWRwb2xlLnYxLkVkZ2VNb2RlEicKCWdyaWRfbW9kZRgmIAEoDjIULmNhZHBvbGUudjEuR3JpZE1vZGUSGwoTZWRnZV9jcmVhc2VfZGVncmVlcxgnIAEoARIVCg1lZGdlX3dpZHRoX3B4GCggASgBEgwKBHhyYXkYKSABKAgSLgoDc3VuGCogASgJQiGKshkdGhszOCwtMTIxLDIwMjYtMTItMjEsMTI6MDAsLTgSIgoNbm9ydGhfZGVncmVlcxgrIAEoAUILirIZBwoFbm9ydGgSGwoHc2VjdGlvbhgsIAEoCUIKirIZBhoEej00MBITCgt0cmFuc3BhcmVudBgtIAEoCBIWCg5zaGFkb3dfY2F0Y2hlchguIAEoCBITCgtzdXBlcnNhbXBsZRgvIAEoDRJBCgZsYXllcnMYMiADKAsyFi5jYWRwb2xlLnYxLkxheWVyU3RhdGVCGYqyGRUKBWxheWVyGgxlbnZlbG9wZT1vZmYaTQoTUGFyYW1PdmVycmlkZXNFbnRyeRILCgNrZXkYASABKAkSJQoFdmFsdWUYAiABKAsyFi5jYWRwb2xlLnYxLlBhcmFtVmFsdWU6AjgBOi2KshkpCidSZW5kZXIgYW4gYW5pbWF0aW9uIGEgZG9jdW1lbnQgZGVjbGFyZXMinAQKD0FuaW1hdGVSZXNwb25zZRIMCgRkYXRhGAEgASgMEg4KBmZvcm1hdBgCIAEoCRIaChJzdWdnZXN0ZWRfZmlsZW5hbWUYAyABKAkSKgoGZnJhbWVzGAQgAygLMhouY2FkcG9sZS52MS5BbmltYXRpb25GcmFtZRINCgV3aWR0aBgFIAEoDRIOCgZoZWlnaHQYBiABKA0SEwoLZnJhbWVfY291bnQYByABKA0SEgoKZHVyYXRpb25fcxgIIAEoARILCgNmcHMYCSABKAESDAoEbG9vcBgKIAEoCBINCgVieXRlcxgLIAEoBBItCgd0aW1pbmdzGAwgASgLMhwuY2FkcG9sZS52MS5BbmltYXRpb25UaW1pbmdzEjMKC2ZyYW1lX3N0YXRzGA0gAygLMh4uY2FkcG9sZS52MS5BbmltYXRpb25GcmFtZVN0YXQSJAoEbWVtbxgOIAEoCzIWLmNhZHBvbGUudjEuTWVtb1JlcG9ydBItCgphbmltYXRpb25zGA8gAygLMhkuY2FkcG9sZS52MS5BbmltYXRpb25JbmZvEisKC2RpYWdub3N0aWNzGBAgAygLMhYuY2FkcG9sZS52MS5EaWFnbm9zdGljEiIKBmNhbWVyYRgRIAEoCzISLmNhZHBvbGUudjEuQ2FtZXJhEicKB2NhbWVyYXMYEiADKAsyFi5jYWRwb2xlLnYxLkNhbWVyYUluZm8iNwoOQW5pbWF0aW9uRnJhbWUSCQoBdBgBIAEoARINCgVpbmRleBgCIAEoDRILCgNwbmcYAyABKAwiiwEKEkFuaW1hdGlvbkZyYW1lU3RhdBIJCgF0GAEgASgBEg8KB2V2YWxfbXMYAiABKAESEQoJcmVuZGVyX21zGAMgASgBEg8KB29wc19ydW4YBCABKA0SDwoHb3BzX2hpdBgFIAEoDRIVCg10ZXNzZWxsYXRpb25zGAYgASgNEg0KBWJha2VzGAcgASgNIogBChBBbmltYXRpb25UaW1pbmdzEhAKCHRvdGFsX21zGAEgASgBEhUKDWZpcnN0X2V2YWxfbXMYAiABKAESFQoNbGF0ZXJfZXZhbF9tcxgDIAEoARIRCglyZW5kZXJfbXMYBCABKAESEQoJZW5jb2RlX21zGAUgASgBEg4KBmZpdF9tcxgGIAEoASKcAQoNU3dlZXBSZXNwb25zZRILCgNwbmcYASABKAwSDQoFd2lkdGgYAiABKA0SDgoGaGVpZ2h0GAMgASgNEgwKBGNvbHMYBCABKA0SJAoFY2VsbHMYBSADKAsyFS5jYWRwb2xlLnYxLlN3ZWVwQ2VsbBIrCgtkaWFnbm9zdGljcxgGIAMoCzIWLmNhZHBvbGUudjEuRGlhZ25vc3RpYyLtAQoJU3dlZXBDZWxsEjEKBnBhcmFtcxgBIAMoCzIhLmNhZHBvbGUudjEuU3dlZXBDZWxsLlBhcmFtc0VudHJ5Eg0KBWxhYmVsGAIgASgJEh4KBGJib3gYAyABKAsyEC5jYWRwb2xlLnYxLkJCb3gSKwoLZGlhZ25vc3RpY3MYBCADKAsyFi5jYWRwb2xlLnYxLkRpYWdub3N0aWMSCgoCb2sYBSABKAgaRQoLUGFyYW1zRW50cnkSCwoDa2V5GAEgASgJEiUKBXZhbHVlGAIgASgLMhYuY2FkcG9sZS52MS5QYXJhbVZhbHVlOgI4ASITChFHZXRTZXNzaW9uUmVxdWVzdCLcAgoHU2Vzc2lvbhIKCgJpZBgBIAEoCRIQCghyZXZpc2lvbhgCIAEoBBItCghkb2N1bWVudBgDIAEoCzIbLmNhZHBvbGUudjEuU2Vzc2lvbkRvY3VtZW50Ei8KCXNlbGVjdGlvbhgEIAEoCzIcLmNhZHBvbGUudjEuU2Vzc2lvblNlbGVjdGlvbhIsCghtZXNzYWdlcxgFIAMoCzIaLmNhZHBvbGUudjEuU2Vzc2lvbk1lc3NhZ2USDwoHY2xpZW50cxgGIAEoDRIWCg5sYXN0X2V2ZW50X3NlcRgHIAEoBBIbChNhc3Npc3RhbnRfbGlzdGVuaW5nGAggASgIEhcKD2NsaWVudF9mYWlsdXJlcxgJIAEoBBIRCgl3ZWJfYnVpbGQYCiABKAk6M4qyGS8KLVRoZSBsaXZlIHNlc3Npb24gb2YgYSBydW5uaW5nIGBjYWRwb2xlIHNlcnZlYCKYAgoPU2Vzc2lvbkRvY3VtZW50EgoKAmlkGAEgASgJEgwKBG5hbWUYAiABKAkSFAoMZGlzcGxheV91bml0GAMgASgJEicKB3NjcmlwdHMYBCADKAsyFi5jYWRwb2xlLnYxLlNjcmlwdEZpbGUSSAoPcGFyYW1fb3ZlcnJpZGVzGAUgAygLMi8uY2FkcG9sZS52MS5TZXNzaW9uRG9jdW1lbnQuUGFyYW1PdmVycmlkZXNFbnRyeRITCgthc3NldF9uYW1lcxgGIAMoCRpNChNQYXJhbU92ZXJyaWRlc0VudHJ5EgsKA2tleRgBIAEoCRIlCgV2YWx1ZRgCIAEoCzIWLmNhZHBvbGUudjEuUGFyYW1WYWx1ZToCOAEicwoQU2Vzc2lvblNlbGVjdGlvbhIoCghzZWxlY3RlZBgBIAMoCzIWLmNhZHBvbGUudjEuU2Vzc2lvblJlZhInCgdob3ZlcmVkGAIgASgLMhYuY2FkcG9sZS52MS5TZXNzaW9uUmVmEgwKBG5vdGUYAyABKAkihgEKClNlc3Npb25SZWYSEgoKZmVhdHVyZV9pZBgBIAEoCRIPCgdib2R5X2lkGAIgASgJEgwKBGtpbmQYAyABKAkSEQoJdG9wb19uYW1lGAQgASgJEjIKC2ZpbmdlcnByaW50GAUgASgLMh0uY2FkcG9sZS52MS5FbnRpdHlGaW5nZXJwcmludCJ7Cg5TZXNzaW9uTWVzc2FnZRILCgNzZXEYASABKAQSDAoEZnJvbRgCIAEoCRIMCgR0ZXh0GAMgASgJEgoKAmF0GAQgASgJEhMKC2F0dGFjaG1lbnRzGAUgASgNEg0KBXN0YXRlGAYgASgJEhAKCGFjdGl2aXR5GAcgASgJIm8KE0dldFNlbGVjdGlvblJlcXVlc3QSLgoIZG9jdW1lbnQYASABKAsyFC5jYWRwb2xlLnYxLkRvY3VtZW50QgaKshkCIAESFQoNc2tpcF9nZW9tZXRyeRgCIAEoCBIRCgl0b2xlcmFuY2UYAyABKAEiSwoUR2V0U2VsZWN0aW9uUmVzcG9uc2USJQoEcmVmcxgBIAMoCzIXLmNhZHBvbGUudjEuUmVzb2x2ZWRSZWYSDAoEbm90ZRgCIAEoCSKqAgoLUmVzb2x2ZWRSZWYSIgoDcmVmGAEgASgLMhUuY2FkcG9sZS52MS5FbnRpdHlSZWYSJQoGc3RhdHVzGAIgASgOMhUuY2FkcG9sZS52MS5SZWZTdGF0dXMSDgoGcmVhc29uGAMgASgJEiYKBGZhY2UYBCABKAsyGC5jYWRwb2xlLnYxLkZhY2VHZW9tZXRyeRImCgRlZGdlGAUgASgLMhguY2FkcG9sZS52MS5FZGdlR2VvbWV0cnkSKAoHZmVhdHVyZRgGIAEoCzIXLmNhZHBvbGUudjEuRmVhdHVyZUluZm8SHgoEYmJveBgHIAEoCzIQLmNhZHBvbGUudjEuQkJveBIVCg1yZXNvbHZlZF9uYW1lGAggASgJEg8KB21hdGNoZXMYCSADKAkihwEKDEZhY2VHZW9tZXRyeRIUCgxzdXJmYWNlX2tpbmQYASABKAkSIgoIY2VudHJvaWQYAiABKAsyEC5jYWRwb2xlLnYxLlZlYzMSIAoGbm9ybWFsGAMgASgLMhAuY2FkcG9sZS52MS5WZWMzEgwKBGFyZWEYBCABKAESDQoFZWRnZXMYBSADKAkigQEKDEVkZ2VHZW9tZXRyeRISCgpjdXJ2ZV9raW5kGAEgASgJEh8KBXN0YXJ0GAIgASgLMhAuY2FkcG9sZS52MS5WZWMzEh0KA2VuZBgDIAEoCzIQLmNhZHBvbGUudjEuVmVjMxIOCgZsZW5ndGgYBCABKAESDQoFZmFjZXMYBSADKAkiyQEKC0ZlYXR1cmVJbmZvEgoKAmlkGAEgASgJEgwKBGtpbmQYAiABKAkSDAoEbmFtZRgDIAEoCRIvCgRhcmdzGAQgAygLMiEuY2FkcG9sZS52MS5GZWF0dXJlSW5mby5BcmdzRW50cnkSJAoEc3BhbhgFIAEoCzIWLmNhZHBvbGUudjEuU291cmNlU3BhbhIOCgZpbnB1dHMYBiADKAkaKwoJQXJnc0VudHJ5EgsKA2tleRgBIAEoCRINCgV2YWx1ZRgCIAEoCToCOAEi9QEKEUV4cGxhaW5SZWZSZXF1ZXN0Ei4KCGRvY3VtZW50GAEgASgLMhQuY2FkcG9sZS52MS5Eb2N1bWVudEIGirIZAiABEh8KCmZlYXR1cmVfaWQYAiABKAlCC4qyGQcQARoDbGlkEh8KCXRvcG9fbmFtZRgDIAEoCUIMirIZCBoGZmFjZTozEhgKBGtpbmQYBCABKAlCCoqyGQYaBGZhY2USDwoHYm9keV9pZBgFIAEoCRIRCgl0b2xlcmFuY2UYBiABKAE6MIqyGSwKKkV4cGxhaW4gd2hhdCBhbiBlbnRpdHkgaXMgYW5kIHdoYXQgbWFkZSBpdCK3AgoSRXhwbGFpblJlZlJlc3BvbnNlEiIKA3JlZhgBIAEoCzIVLmNhZHBvbGUudjEuRW50aXR5UmVmEiUKBnN0YXR1cxgCIAEoDjIVLmNhZHBvbGUudjEuUmVmU3RhdHVzEg4KBnJlYXNvbhgDIAEoCRIlCgVjaGFpbhgEIAMoCzIWLmNhZHBvbGUudjEuUHJvdmVuYW5jZRIoCglnZW5lcmF0b3IYBSABKAsyFS5jYWRwb2xlLnYxLkdlbmVyYXRvchIUCgxjb250cmlidXRvcnMYBiADKAkSDwoHc3VtbWFyeRgHIAEoCRImCgRmYWNlGAggASgLMhguY2FkcG9sZS52MS5GYWNlR2VvbWV0cnkSJgoEZWRnZRgJIAEoCzIYLmNhZHBvbGUudjEuRWRnZUdlb21ldHJ5It0BCgpQcm92ZW5hbmNlEhIKCmZlYXR1cmVfaWQYASABKAkSDAoEa2luZBgCIAEoCRIMCgRyb2xlGAMgASgJEiQKBHNwYW4YBCABKAsyFi5jYWRwb2xlLnYxLlNvdXJjZVNwYW4SDgoGaW5wdXRzGAUgAygJEi4KBGFyZ3MYBiADKAsyIC5jYWRwb2xlLnYxLlByb3ZlbmFuY2UuQXJnc0VudHJ5EgwKBG5hbWUYByABKAkaKwoJQXJnc0VudHJ5EgsKA2tleRgBIAEoCRINCgV2YWx1ZRgCIAEoCToCOAEihwEKCUdlbmVyYXRvchISCgpmZWF0dXJlX2lkGAEgASgJEhIKCmxvY2FsX25hbWUYAiABKAkSEQoJc2tldGNoX2lkGAMgASgJEgwKBGxvb3AYBCABKA0SCwoDc2VnGAUgASgNEiQKBHNwYW4YBiABKAsyFi5jYWRwb2xlLnYxLlNvdXJjZVNwYW4imAEKEFNldFNjcmlwdFJlcXVlc3QSHwoEcGF0aBgBIAEoCUIRirIZDRABGgltYWluLnN0YXISFgoGc291cmNlGAIgASgJQgaKshkCMAESHQoNYmFzZV9yZXZpc2lvbhgDIAEoBEIGirIZAjABOiyKshkoCiZSZXBsYWNlIG9uZSBzY3JpcHQgZmlsZSBpbiB0aGUgc2Vzc2lvbiKoAQoRU2V0U2NyaXB0UmVzcG9uc2USEAoIcmV2aXNpb24YASABKAQSKwoLZGlhZ25vc3RpY3MYAiADKAsyFi5jYWRwb2xlLnYxLkRpYWdub3N0aWMSEAoIZmVhdHVyZXMYAyADKAkSEAoIY29uZmxpY3QYBCABKAgSGAoQY3VycmVudF9yZXZpc2lvbhgFIAEoBBIWCg5jdXJyZW50X3NvdXJjZRgGIAEoCSK/AQoKU2NyaXB0RWRpdBIiCgdvbGRfc3RyGAEgASgJQhGKshkNGgtoZWlnaHQgPSAxMBIiCgduZXdfc3RyGAIgASgJQhGKshkNGgtoZWlnaHQgPSAyMBITCgtyZXBsYWNlX2FsbBgDIAEoCBIcCgpzdGFydF9saW5lGAQgASgNQgiKshkEGgIxMhIaCghlbmRfbGluZRgFIAEoDUIIirIZBBoCMTQSGgoHZmVhdHVyZRgGIAEoCUIJirIZBRoDbGlkIu8BChFFZGl0U2NyaXB0UmVxdWVzdBIfCgRwYXRoGAEgASgJQhGKshkNEAEaCW1haW4uc3RhchJpCgVlZGl0cxgCIAMoCzIWLmNhZHBvbGUudjEuU2NyaXB0RWRpdEJCirIZPgoEZWRpdBo0eyJvbGRfc3RyIjogImhlaWdodCA9IDEwIiwgIm5ld19zdHIiOiAiaGVpZ2h0ID0gMjAifTABEh0KDWJhc2VfcmV2aXNpb24YAyABKARCBoqyGQIwATovirIZKwopRWRpdCBwYXJ0IG9mIGEgc2NyaXB0IGZpbGUgaW4gdGhlIHNlc3Npb24iyAEKEkVkaXRTY3JpcHRSZXNwb25zZRIQCghyZXZpc2lvbhgBIAEoBBIrCgtkaWFnbm9zdGljcxgCIAMoCzIWLmNhZHBvbGUudjEuRGlhZ25vc3RpYxIQCghmZWF0dXJlcxgDIAMoCRIQCghjb25mbGljdBgEIAEoCBIYChBjdXJyZW50X3JldmlzaW9uGAUgASgEEhYKDmN1cnJlbnRfc291cmNlGAYgASgJEgwKBGRpZmYYByABKAkSDwoHYXBwbGllZBgIIAEoDSLqAQoQU2V0UGFyYW1zUmVxdWVzdBJVCglvdmVycmlkZXMYASADKAsyKy5jYWRwb2xlLnYxLlNldFBhcmFtc1JlcXVlc3QuT3ZlcnJpZGVzRW50cnlCFYqyGREKBXBhcmFtGgh3YWxsPTIuNBINCgVjbGVhchgCIAMoCRpICg5PdmVycmlkZXNFbnRyeRILCgNrZXkYASABKAkSJQoFdmFsdWUYAiABKAsyFi5jYWRwb2xlLnYxLlBhcmFtVmFsdWU6AjgBOiaKshkiCiBTZXQgb3IgY2xlYXIgcGFyYW1ldGVyIG92ZXJyaWRlcyLAAQoRU2V0UGFyYW1zUmVzcG9uc2USEAoIcmV2aXNpb24YASABKAQSSgoPcGFyYW1fb3ZlcnJpZGVzGAIgAygLMjEuY2FkcG9sZS52MS5TZXRQYXJhbXNSZXNwb25zZS5QYXJhbU92ZXJyaWRlc0VudHJ5Gk0KE1BhcmFtT3ZlcnJpZGVzRW50cnkSCwoDa2V5GAEgASgJEiUKBXZhbHVlGAIgASgLMhYuY2FkcG9sZS52MS5QYXJhbVZhbHVlOgI4ASKpAQoTU2V0U2VsZWN0aW9uUmVxdWVzdBIoCghzZWxlY3RlZBgBIAMoCzIWLmNhZHBvbGUudjEuU2Vzc2lvblJlZhInCgdob3ZlcmVkGAIgASgLMhYuY2FkcG9sZS52MS5TZXNzaW9uUmVmEgwKBG5vdGUYAyABKAk6MYqyGS0KK1BvaW50IHRoZSB1c2VyIGF0IHNvbWV0aGluZyBpbiB0aGUgdmlld3BvcnQiOgoUU2V0U2VsZWN0aW9uUmVzcG9uc2USEAoIcmV2aXNpb24YASABKAQSEAoIc2VsZWN0ZWQYAiABKA0iJgoMUmVwbHlSZXF1ZXN0EhYKBHRleHQYASABKAlCCIqyGQQQATABIhwKDVJlcGx5UmVzcG9uc2USCwoDc2VxGAEgASgEIrkBChJTdHJlYW1SZXBseVJlcXVlc3QSFgoDc2VxGAEgASgEQgmKshkFCgNzZXESFgoGYXBwZW5kGAIgASgJQgaKshkCEAESEAoIYWN0aXZpdHkYAyABKAkSJwoGZmluaXNoGAQgASgOMhcuY2FkcG9sZS52MS5SZXBseUZpbmlzaDo4irIZNAoyUG9zdCBhIG1lc3NhZ2UgYXMgdGhlIGFzc2lzdGFudCwgYSBwaWVjZSBhdCBhIHRpbWUiVAoTU3RyZWFtUmVwbHlSZXNwb25zZRILCgNzZXEYASABKAQSDQoFc3RhdGUYAiABKAkSDgoGbGVuZ3RoGAMgASgEEhEKCXRocm90dGxlZBgEIAEoCCKKAQoSUG9zdE1lc3NhZ2VSZXF1ZXN0EhYKBHRleHQYASABKAlCCIqyGQQQATABEjoKC2F0dGFjaG1lbnRzGAIgAygLMh0uY2FkcG9sZS52MS5NZXNzYWdlQXR0YWNobWVudEIGirIZAiABOiCKshkcChpQb3N0IGEgbWVzc2FnZSBhcyB0aGUgdXNlciJqChFNZXNzYWdlQXR0YWNobWVudBIMCgRraW5kGAEgASgJEgsKA3BuZxgCIAEoDBInCgZjYW1lcmEYAyABKAsyFy5nb29nbGUucHJvdG9idWYuU3RydWN0EhEKCWFubm90YXRlZBgEIAEoCCJCChNQb3N0TWVzc2FnZVJlc3BvbnNlEisKB21lc3NhZ2UYASABKAsyGi5jYWRwb2xlLnYxLlNlc3Npb25NZXNzYWdlIjkKC1NhdmVSZXF1ZXN0OiqKshkmCiRXcml0ZSB0aGUgc2Vzc2lvbidzIGRvY3VtZW50IHRvIGRpc2siHAoMU2F2ZVJlc3BvbnNlEgwKBHBhdGgYASABKAkiyQEKD1J1blRlc3RzUmVxdWVzdBIuCghkb2N1bWVudBgBIAEoCzIULmNhZHBvbGUudjEuRG9jdW1lbnRCBoqyGQIgARIpCgVuYW1lcxgCIAMoCUIairIZFgoEbmFtZRoOZml0cyB0aGUgc2hlbGYSEQoJdG9sZXJhbmNlGAMgASgBEhkKEWJvb2xlYW5fdG9sZXJhbmNlGAQgASgBEg8KB3RocmVhZHMYBSABKA06HIqyGRgKFlJ1biBhIGRvY3VtZW50J3MgdGVzdHMipgEKEFJ1blRlc3RzUmVzcG9uc2USJwoHcmVzdWx0cxgBIAMoCzIWLmNhZHBvbGUudjEuVGVzdFJlc3VsdBIrCgtkaWFnbm9zdGljcxgCIAMoCzIWLmNhZHBvbGUudjEuRGlhZ25vc3RpYxIKCgJvaxgDIAEoCBIOCgZwYXNzZWQYBCABKA0SDgoGZmFpbGVkGAUgASgNEhAKCGRlY2xhcmVkGAYgASgNIpYCCgpUZXN0UmVzdWx0EgwKBG5hbWUYASABKAkSCgoCb2sYAiABKAgSDwoHbWVzc2FnZRgDIAEoCRIkCgRzcGFuGAQgASgLMhYuY2FkcG9sZS52MS5Tb3VyY2VTcGFuEjIKBnBhcmFtcxgFIAMoCzIiLmNhZHBvbGUudjEuVGVzdFJlc3VsdC5QYXJhbXNFbnRyeRINCgVsYWJlbBgGIAEoCRISCgV2YWx1ZRgHIAEoAUgAiAEBEg8KB2V2YWxfbXMYCCABKAEaRQoLUGFyYW1zRW50cnkSCwoDa2V5GAEgASgJEiUKBXZhbHVlGAIgASgLMhYuY2FkcG9sZS52MS5QYXJhbVZhbHVlOgI4AUIICgZfdmFsdWUiuwQKDFBsYXRlUmVxdWVzdBIuCghkb2N1bWVudBgBIAEoCzIULmNhZHBvbGUudjEuRG9jdW1lbnRCBoqyGQIgARIQCghib2R5X2lkcxgCIAMoCRInCgtsYXllcl9uYW1lcxgDIAMoCUISirIZDgoFbGF5ZXIaBXBhcnRzEh0KFWluY2x1ZGVfaGlkZGVuX2xheWVycxgEIAEoCBIgCgdwcmludGVyGAUgASgJQg+KshkLGgliYW1idV9oMmMSFAoDYW1zGAYgASgNQgeKshkDGgE0EiUKCHZhcmlhbnRzGAcgAygJQhOKshkPCgd2YXJpYW50GgRtb25vEhgKB3NwYWNpbmcYCCABKAFCB4qyGQMaATQSGgoJam9pbnRfZ2FwGAkgASgBQgeKshkDGgExEhwKCm1heF9icmlkZ2UYCiABKAFCCIqyGQQaAjEwEjEKDWNvdXBvbl9sYXllcnMYCyADKAlCGoqyGRYKDGNvdXBvbi1sYXllchoGY291cG9uEhYKDnNjcmlwdF9vYmplY3RzGAwgASgIEiMKCHRlbXBsYXRlGA0gASgJQhGKshkNGgtoMmNfcGxhLjNtZhIRCgl0b2xlcmFuY2UYDiABKAESGQoRYm9vbGVhbl90b2xlcmFuY2UYDyABKAESFAoMcGxhdGVfaW1hZ2VzGBAgASgIEhgKBXRpdGxlGBEgASgJQgmKshkFGgN0b3k6IIqyGRwKGlBsYXRlIGEgbW9kZWwgZm9yIHByaW50aW5nIvEBCg1QbGF0ZVJlc3BvbnNlEgwKBGRhdGEYASABKAwSDgoGcmVwb3J0GAIgASgJEigKBnBsYXRlcxgDIAMoCzIYLmNhZHBvbGUudjEuUGxhbm5lZFBsYXRlEigKBnBpZWNlcxgEIAMoCzIYLmNhZHBvbGUudjEuUGxhbm5lZFBpZWNlEisKC2RpYWdub3N0aWNzGAUgAygLMhYuY2FkcG9sZS52MS5EaWFnbm9zdGljEiwKDHBsYXRlX2ltYWdlcxgGIAMoCzIWLmNhZHBvbGUudjEuUGxhdGVJbWFnZRITCgtwbGF0ZV9zaGVldBgHIAEoDCJ4CgxQbGFubmVkUGxhdGUSDAoEbmFtZRgBIAEoCRIPCgd2YXJpYW50GAIgASgJEg4KBnBpZWNlcxgDIAMoCRIRCglmaWxhbWVudHMYBCADKAkSFgoOY29sb3VyX2NoYW5nZXMYBSABKA0SDgoGbGF5ZXJzGAYgASgNIrEBCgxQbGFubmVkUGllY2USDAoEbmFtZRgBIAEoCRINCgVwbGF0ZRgCIAEoCRIMCgRraW5kGAMgASgJEhMKC29yaWVudGF0aW9uGAQgASgJEhQKDHN1cHBvcnRfYXJlYRgFIAEoARIdChVtb2RlbGxlZF9zdXBwb3J0X2FyZWEYBiABKAESDAoEc3BpbhgHIAEoARIOCgZyZWFzb24YCCABKAkSDgoGYm9kaWVzGAkgAygJIsUHChFQcmludENoZWNrUmVxdWVzdBIuCghkb2N1bWVudBgBIAEoCzIULmNhZHBvbGUudjEuRG9jdW1lbnRCBoqyGQIgARIQCghib2R5X2lkcxgCIAMoCRIfCgdwcmludGVyGAwgASgJQg6KshkKGghiYW1idV9hMRIbCghtaW5fd2FsbBgDIAEoAUIJirIZBRoDMC44EhkKBm5venpsZRgEIAEoAUIJirIZBRoDMC40EhsKCnBlcmltZXRlcnMYBSABKAFCB4qyGQMaATISNAoUbWF4X292ZXJoYW5nX2RlZ3JlZXMYBiABKAFCFoqyGRIKDG1heC1vdmVyaGFuZxoCNDUSFgoDYmVkGAcgAygBQgmKshkFGgMyNTYSHwoPYnVpbGRfZGlyZWN0aW9uGAggAygBQgaKshkCIAESDgoGb3JpZW50GAkgASgIEhEKCXRvbGVyYW5jZRgKIAEoARIZChFib29sZWFuX3RvbGVyYW5jZRgLIAEoARIfCgxsYXllcl9oZWlnaHQYDSABKAFCCYqyGQUaAzAuMhIcCghtYXRlcmlhbBgOIAEoCUIKirIZBhoEUEVURxIhCgliZWRfc2hhcGUYDyABKAlCDoqyGQoaCGNpcmN1bGFyEicKC2xheWVyX25hbWVzGBAgAygJQhKKshkOCgVsYXllchoFd2FsbHMSHQoVaW5jbHVkZV9oaWRkZW5fbGF5ZXJzGBEgASgIEhwKCm1heF9icmlkZ2UYEiABKAFCCIqyGQQaAjEwEiIKEW1heF9vdmVyaGFuZ19hcmVhGBMgASgBQgeKshkDGgE0Eh8KDm1heF9jYW50aWxldmVyGBQgASgBQgeKshkDGgEzEiQKE21heF9ob3Jpem9udGFsX2hvbGUYFSABKAFCB4qyGQMaATcSHQoKbWluX2lzbGFuZBgWIAEoAUIJirIZBRoDMC4yEiAKD21heF9zbGVuZGVybmVzcxgXIAEoAUIHirIZAxoBOBIeCg1zbGVuZGVyX3dpZHRoGBggASgBQgeKshkDGgEzEiMKEm1heF9oZWlnaHRfdG9fYmFzZRgZIAEoAUIHirIZAxoBNhIUCgxzdXBwb3J0X2ZyZWUYGiABKAgSLgoTb3B0aW1hbF9vcmllbnRhdGlvbhgbIAEoCEIRirIZDQoLb3JpZW50YXRpb24SJgoVb3JpZW50YXRpb25fdG9sZXJhbmNlGBwgASgBQgeKshkDGgE1OiSKshkgCh5DaGVjayBhIG1vZGVsIGZvciBwcmludGFiaWxpdHkiegoSUHJpbnRDaGVja1Jlc3BvbnNlEisKBmJvZGllcxgBIAMoCzIbLmNhZHBvbGUudjEuQm9keVByaW50UmVwb3J0EisKC2RpYWdub3N0aWNzGAIgAygLMhYuY2FkcG9sZS52MS5EaWFnbm9zdGljEgoKAm9rGAMgASgIIpEKCg9Cb2R5UHJpbnRSZXBvcnQSDwoHYm9keV9pZBgBIAEoCRIMCgRuYW1lGAIgASgJEhEKCXRyaWFuZ2xlcxgDIAEoDRISCgp2b2x1bWVfbW0zGAQgASgBEhAKCGFyZWFfbW0yGAUgASgBEh4KBGJib3gYBiABKAsyEC5jYWRwb2xlLnYxLkJCb3gSEgoKd2F0ZXJ0aWdodBgHIAEoCBIWCg5ib3VuZGFyeV9lZGdlcxgIIAEoDRIkCgVob2xlcxgJIAMoCzIVLmNhZHBvbGUudjEuUHJpbnRIb2xlEhoKEm5vbl9tYW5pZm9sZF9lZGdlcxgKIAEoDRIVCg1mbGlwcGVkX2VkZ2VzGAsgASgNEhgKEGludmVydGVkX25vcm1hbHMYDCABKAgSGgoSc2VsZl9pbnRlcnNlY3Rpb25zGA0gASgNEhwKFGRlZ2VuZXJhdGVfdHJpYW5nbGVzGA4gASgNEhMKC21pbl93YWxsX21tGA8gASgBEhUKDXRoaW5fYXJlYV9tbTIYECABKAESHAoUdW5zdXBwb3J0ZWRfYXJlYV9tbTIYESABKAESHAoUbWF4X292ZXJoYW5nX2RlZ3JlZXMYEiABKAESHAoUYmVkX2NvbnRhY3RfYXJlYV9tbTIYEyABKAESIQoHc2l6ZV9tbRgUIAEoCzIQLmNhZHBvbGUudjEuVmVjMxIQCghmaXRzX2JlZBgVIAEoCBIYChBmaXRzX2JlZF9yb3RhdGVkGBYgASgIEicKDWJlZF9leGNlc3NfbW0YFyABKAsyEC5jYWRwb2xlLnYxLlZlYzMSJgoGaXNzdWVzGBggAygLMhYuY2FkcG9sZS52MS5QcmludElzc3VlEjEKC29yaWVudGF0aW9uGBkgASgLMhwuY2FkcG9sZS52MS5QcmludE9yaWVudGF0aW9uEgoKAm9rGBogASgIEg8KB3N1bW1hcnkYGyABKAkSDwoHcHJvZmlsZRgcIAEoCRIOCgZtYXNzX2cYHSABKAESDgoGbGF5ZXJzGB4gASgNEiUKBXNwbGl0GB8gASgLMhYuY2FkcG9sZS52MS5QcmludFNwbGl0Eg0KBXNvbGlkGCAgASgIEhYKDmludGVybmFsX2ZhY2VzGCEgASgNEhIKCmNvbXBvbmVudHMYIiABKA0SDQoFZ2VudXMYIyABKAUSFgoOZnVzZWRfYm9keV9pZHMYJCADKAkSFwoPYnJpZGdlX2FyZWFfbW0yGCUgASgBEhkKEWxvbmdlc3RfYnJpZGdlX21tGCYgASgBEg8KB2JyaWRnZXMYJyABKA0SFQoNbWF4X2JyaWRnZV9tbRgoIAEoARIYChBzdXBwb3J0X2FyZWFfbW0yGCkgASgBEhwKFGxhcmdlc3Rfb3ZlcmhhbmdfbW0yGCogASgBEh0KFWxvbmdlc3RfY2FudGlsZXZlcl9tbRgrIAEoARIPCgdpc2xhbmRzGCwgASgNEhkKEWRlZXBlc3RfaXNsYW5kX21tGC0gASgBEhgKEGhvcml6b250YWxfaG9sZXMYLiABKA0SEgoKd2lkZV9ob2xlcxgvIAEoDRIPCgdzbGVuZGVyGDAgASgNEhMKC3NsZW5kZXJuZXNzGDEgASgBEhwKFGZpcnN0X2xheWVyX2FyZWFfbW0yGDIgASgBEhYKDmhlaWdodF90b19iYXNlGDMgASgBEhQKDHN1cHBvcnRfZnJlZRg0IAEoCCJGCgpQcmludFNwbGl0EgwKBGF4aXMYASABKAkSDAoEY3V0cxgCIAMoARIOCgZwaWVjZXMYAyABKA0SDAoEY2FsbBgEIAEoCSJrCgpQcmludElzc3VlEg0KBWNoZWNrGAEgASgJEhAKCHNldmVyaXR5GAIgASgJEg8KB21lc3NhZ2UYAyABKAkSHAoCYXQYBCABKAsyEC5jYWRwb2xlLnYxLlZlYzMSDQoFdmFsdWUYBSABKAEicgoJUHJpbnRIb2xlEiAKBmNlbnRlchgBIAEoCzIQLmNhZHBvbGUudjEuVmVjMxIUCgxwZXJpbWV0ZXJfbW0YAiABKAESDQoFZWRnZXMYAyABKA0SHgoEYmJveBgEIAEoCzIQLmNhZHBvbGUudjEuQkJveCK0AQoQUHJpbnRPcmllbnRhdGlvbhIeCgRkb3duGAEgASgLMhAuY2FkcG9sZS52MS5WZWMzEg0KBWxhYmVsGAIgASgJEg8KB2NoYW5nZWQYAyABKAgSHAoUdW5zdXBwb3J0ZWRfYXJlYV9tbTIYBCABKAESHAoUYmVkX2NvbnRhY3RfYXJlYV9tbTIYBSABKAESEQoJaGVpZ2h0X21tGAYgASgBEhEKCXJlYXNvbmluZxgHIAEoCSLwAgoOSW5zcGVjdFJlcXVlc3QSLgoIZG9jdW1lbnQYASABKAsyFC5jYWRwb2xlLnYxLkRvY3VtZW50QgaKshkCIAESKQoIYm9keV9pZHMYAiADKAlCF4qyGRMKBGJvZHkaC2V4dHJ1ZGVfMSMwEl4KD3BhcmFtX292ZXJyaWRlcxgDIAMoCzIuLmNhZHBvbGUudjEuSW5zcGVjdFJlcXVlc3QuUGFyYW1PdmVycmlkZXNFbnRyeUIVirIZEQoFcGFyYW0aCHdhbGw9Mi40EhEKCXRvbGVyYW5jZRgEIAEoARIZChFib29sZWFuX3RvbGVyYW5jZRgFIAEoARpNChNQYXJhbU92ZXJyaWRlc0VudHJ5EgsKA2tleRgBIAEoCRIlCgV2YWx1ZRgCIAEoCzIWLmNhZHBvbGUudjEuUGFyYW1WYWx1ZToCOAE6JoqyGSIKIEluc3BlY3QgdGhlIGJvZGllcyBvZiBhIGRvY3VtZW50ItUBCg9JbnNwZWN0UmVzcG9uc2USKgoGYm9kaWVzGAEgAygLMhouY2FkcG9sZS52MS5Cb2R5SW5zcGVjdGlvbhIwCghmZWF0dXJlcxgCIAMoCzIeLmNhZHBvbGUudjEuRmVhdHVyZURpYWdub3N0aWNzEgoKAm9rGAMgASgIEicKCWluc3RhbmNlcxgEIAMoCzIULmNhZHBvbGUudjEuSW5zdGFuY2USLwoNZGlzcGxheV91bml0cxgFIAEoCzIYLmNhZHBvbGUudjEuRGlzcGxheVVuaXRzIokBChJGZWF0dXJlRGlhZ25vc3RpY3MSEgoKZmVhdHVyZV9pZBgBIAEoCRIMCgRraW5kGAIgASgJEiQKBHNwYW4YAyABKAsyFi5jYWRwb2xlLnYxLlNvdXJjZVNwYW4SKwoLZGlhZ25vc3RpY3MYBCADKAsyFi5jYWRwb2xlLnYxLkRpYWdub3N0aWMiggUKDkJvZHlJbnNwZWN0aW9uEg8KB2JvZHlfaWQYASABKAkSDAoEbmFtZRgCIAEoCRISCgpmZWF0dXJlX2lkGAMgASgJEgwKBGtpbmQYBCABKAkSDQoFZmFjZXMYBSABKA0SDQoFZWRnZXMYBiABKA0SEQoJdHJpYW5nbGVzGAcgASgNEhIKCnRoaW5fZmFjZXMYCCABKA0SHgoEYmJveBgJIAEoCzIQLmNhZHBvbGUudjEuQkJveBISCgp2b2x1bWVfbW0zGAogASgBEhAKCGFyZWFfbW0yGAsgASgBEicKBXNvbGlkGAwgASgLMhguY2FkcG9sZS52MS5Tb2xpZFZlcmRpY3QSFwoPbWluX2FsdGl0dWRlX21tGA0gASgBEikKD21pbl9hbHRpdHVkZV9hdBgOIAEoCzIQLmNhZHBvbGUudjEuVmVjMxIiChptYW5pZm9sZF9zZWxmX2ludGVyc2VjdGluZxgPIAEoCBIeChZtYW5pZm9sZF92ZXJkaWN0X2tub3duGBAgASgIEg8KB3N1bW1hcnkYESABKAkSLwoIc3VyZmFjZXMYEiADKAsyHS5jYWRwb2xlLnYxLlN1cmZhY2VDbGFzc0NvdW50EhEKCXRvbGVyYW5jZRgTIAEoARIYChB0b2xlcmFuY2Vfc291cmNlGBQgASgJEhYKDnRlc3NlbGxhdGVkX2F0GBUgASgBEhgKC2Nob3JkX2Vycm9yGBYgASgBSACIAQESGAoQY2hvcmRfZXJyb3JfZmFjZRgXIAEoCRIoCg5jaG9yZF9lcnJvcl9hdBgYIAEoCzIQLmNhZHBvbGUudjEuVmVjM0IOCgxfY2hvcmRfZXJyb3IiMAoRU3VyZmFjZUNsYXNzQ291bnQSDAoEa2luZBgBIAEoCRINCgVmYWNlcxgCIAEoDSLjAQoMU29saWRWZXJkaWN0EhAKCGlzX3NvbGlkGAEgASgIEg8KB3N1bW1hcnkYAiABKAkSJwoHZmFpbGluZxgDIAMoCzIWLmNhZHBvbGUudjEuU29saWRDaGVjaxINCgVmb2xkcxgEIAEoDRIqCgp3b3JzdF9mb2xkGAUgASgLMhYuY2FkcG9sZS52MS5Gb2xkUmVwb3J0EhUKDXBhaXJzX3NraXBwZWQYBiABKAgSEgoKY29tcG9uZW50cxgHIAEoDRINCgVnZW51cxgIIAEoBRISCgpkZWdlbmVyYXRlGAkgASgNIlkKClNvbGlkQ2hlY2sSDQoFY2hlY2sYASABKAkSDQoFY291bnQYAiABKA0SHAoCYXQYAyABKAsyEC5jYWRwb2xlLnYxLlZlYzMSDwoHbWVzc2FnZRgEIAEoCSJrCgpGb2xkUmVwb3J0EhwKAmF0GAEgASgLMhAuY2FkcG9sZS52MS5WZWMzEhIKCnRpcF9nYXBfbW0YAiABKAESCwoDZG90GAMgASgBEg4KBmZhY2VfYRgEIAEoCRIOCgZmYWNlX2IYBSABKAkibAoUR2V0QXR0YWNobWVudFJlcXVlc3QSGAoDc2VxGAEgASgEQguKshkHEAEaATMwARIWCgVpbmRleBgCIAEoDUIHirIZAxoBMDoiirIZHgocRmV0Y2ggYSBtZXNzYWdlJ3Mgc2NyZWVuc2hvdCKNAQoVR2V0QXR0YWNobWVudFJlc3BvbnNlEgsKA3BuZxgBIAEoDBIMCgRraW5kGAIgASgJEhEKCWFubm90YXRlZBgDIAEoCBINCgV3aWR0aBgEIAEoDRIOCgZoZWlnaHQYBSABKA0SJwoGY2FtZXJhGAYgASgLMhcuZ29vZ2xlLnByb3RvYnVmLlN0cnVjdCJ2CgpBY2tSZXF1ZXN0EiIKBmNsaWVudBgBIAEoCUISirIZDhABGghtY3A6NDcxMTABEhoKBmN1cnNvchgCIAEoBEIKirIZBhoCNDIwATooirIZJAoiQWR2YW5jZSB0aGlzIGNsaWVudCdzIGV2ZW50IGN1cnNvciJaCgtBY2tSZXNwb25zZRIOCgZjbGllbnQYASABKAkSDgoGY3Vyc29yGAIgASgEEhYKDmxhc3RfZXZlbnRfc2VxGAMgASgEEhMKC2Vwb2NoX3Jlc2V0GAQgASgIImUKEUxpc3RBc3NldHNSZXF1ZXN0Ei4KCGRvY3VtZW50GAEgASgLMhQuY2FkcG9sZS52MS5Eb2N1bWVudEIGirIZAiABOiCKshkcChpMaXN0IHRoZSBkb2N1bWVudCdzIGFzc2V0cyKBAQoSTGlzdEFzc2V0c1Jlc3BvbnNlEiEKBmFzc2V0cxgBIAMoCzIRLmNhZHBvbGUudjEuQXNzZXQSEwoLdG90YWxfYnl0ZXMYAiABKAQSFwoPbWF4X2Fzc2V0X2J5dGVzGAMgASgEEhoKEm1heF9kb2N1bWVudF9ieXRlcxgEIAEoBCKgAQoPQWRkQXNzZXRSZXF1ZXN0EiAKBG5hbWUYASABKAlCEoqyGQ4QARoIcGxhbi5keGYwARIUCgRkYXRhGAIgASgMQgaKshkCMAESEwoLc291cmNlX3BhdGgYAyABKAkSFQoNYmFzZV9yZXZpc2lvbhgEIAEoBDopirIZJQojQWRkIGEgZmlsZSB0byB0aGUgZG9jdW1lbnQncyBhc3NldHMicgoQQWRkQXNzZXRSZXNwb25zZRIgCgVhc3NldBgBIAEoCzIRLmNhZHBvbGUudjEuQXNzZXQSEAoIcmV2aXNpb24YAiABKAQSEAoIY29uZmxpY3QYAyABKAgSGAoQY3VycmVudF9yZXZpc2lvbhgEIAEoBCKQAQoPR2V0QXNzZXRSZXF1ZXN0Ei4KCGRvY3VtZW50GAEgASgLMhQuY2FkcG9sZS52MS5Eb2N1bWVudEIGirIZAiABEiAKBG5hbWUYAiABKAlCEoqyGQ4QARoIcGxhbi5keGYwARIVCg1tZXRhZGF0YV9vbmx5GAMgASgIOhSKshkQCg5SZWFkIG9uZSBhc3NldCJZChBHZXRBc3NldFJlc3BvbnNlEiAKBWFzc2V0GAEgASgLMhEuY2FkcG9sZS52MS5Bc3NldBIMCgRkYXRhGAIgASgMEhUKDWJ5dGVzX21pc3NpbmcYAyABKAgi+QcKDkRyYXdpbmdSZXF1ZXN0Ei4KCGRvY3VtZW50GAEgASgLMhQuY2FkcG9sZS52MS5Eb2N1bWVudEIGirIZAiABEh4KBHZpZXcYAiABKAlCEIqyGQwaCnRocmVlLXZpZXcSGQoGZm9ybWF0GAMgASgJQgmKshkFGgNzdmcSGAoFc2NhbGUYBCABKAlCCYqyGQUaAzE6MhIXCgVzaGVldBgFIAEoCUIIirIZBBoCQTMSHgoKZGltZW5zaW9ucxgGIAEoCUIKirIZBhoEYXV0bxIcCgZoaWRkZW4YByABKAlCDIqyGQgaBmRhc2hlZBIXCgV1bml0cxgIIAEoCUIIirIZBBoCbW0SHAoFdGl0bGUYCSABKAlCDYqyGQkaB0JyYWNrZXQSIwoIYm9keV9pZHMYCiADKAlCEYqyGQ0aC2V4dHJ1ZGVfMSMwEhEKCXRvbGVyYW5jZRgLIAEoARJeCg9wYXJhbV9vdmVycmlkZXMYDCADKAsyLi5jYWRwb2xlLnYxLkRyYXdpbmdSZXF1ZXN0LlBhcmFtT3ZlcnJpZGVzRW50cnlCFYqyGREKBXBhcmFtGgh3YWxsPTIuNBIZCgRsb29rGA0gASgJQguKshkHGgVjb2xvchIdCgdzZWN0aW9uGA4gASgJQgyKshkIGgZ6PTE3MDASMQoIc3VidGl0bGUYDyABKAlCH4qyGRsaGUdyb3VuZCBmbG9vciBhbmQgdGhlIGRlY2sSNgoEbm90ZRgQIAEoCUIoirIZJBoiTm9ydGggdXA7IHRoZSBzdHJlZXQgYXQgdGhlIGJvdHRvbRIsCgZsZWdlbmQYESADKAlCHIqyGRgaFiNlMzliNzI9ZXh0ZXJpb3Igd2FsbHMSNgoGbGFiZWxzGBIgAygJQiaKshkiCgVsYWJlbBoZMjYwMCw0MDAwLDUzMD1GYW1pbHkgcm9vbRJBCgZsYXllcnMYEyADKAsyFi5jYWRwb2xlLnYxLkxheWVyU3RhdGVCGYqyGRUKBWxheWVyGgxlbnZlbG9wZT1vZmYSIgoNbm9ydGhfZGVncmVlcxgUIAEoAUILirIZBwoFbm9ydGgSKgoEY3JvcBgVIAEoCUIcirIZGBoWLTI3MDAsLTkwMCwxNjUwMCwyMjAwMBIcCgptb2RlbF91bml0GBYgASgJQgiKshkEGgJmdBpNChNQYXJhbU92ZXJyaWRlc0VudHJ5EgsKA2tleRgBIAEoCRIlCgV2YWx1ZRgCIAEoCzIWLmNhZHBvbGUudjEuUGFyYW1WYWx1ZToCOAE6MYqyGS0KK0RyYXcgYSBkb2N1bWVudCBhcyBhIGRpbWVuc2lvbmVkIDJEIGRyYXdpbmcipQEKD0RyYXdpbmdSZXNwb25zZRIMCgRkYXRhGAEgASgMEg4KBmZvcm1hdBgCIAEoCRIaChJzdWdnZXN0ZWRfZmlsZW5hbWUYAyABKAkSDQoFc2NhbGUYBCABKAkSDQoFaXRlbXMYBSABKA0SDQoFbm90ZXMYBiADKAkSKwoLZGlhZ25vc3RpY3MYByADKAsyFi5jYWRwb2xlLnYxLkRpYWdub3N0aWMirgEKEVdhaXRFdmVudHNSZXF1ZXN0EhYKBWFmdGVyGAEgASgEQgeKshkDGgEwEh8KCnRpbWVvdXRfbXMYAiABKA1CC4qyGQcaBTMwMDAwEhwKBWtpbmRzGAMgAygJQg2KshkJGgdtZXNzYWdlEh4KBmNsaWVudBgEIAEoCUIOirIZChoIbWNwOjQ3MTE6IoqyGR4KHExvbmctcG9sbCBmb3Igc2Vzc2lvbiBldmVudHMibQoSV2FpdEV2ZW50c1Jlc3BvbnNlEiEKBmV2ZW50cxgBIAMoCzIRLmNhZHBvbGUudjEuRXZlbnQSDgoGY3Vyc29yGAIgASgEEhEKCXRydW5jYXRlZBgDIAEoCBIRCglmaXJzdF9zZXEYBCABKAQiYQoFRXZlbnQSCwoDc2VxGAEgASgEEgoKAmF0GAIgASgJEgwKBGtpbmQYAyABKAkSJQoEZGF0YRgEIAEoCzIXLmdvb2dsZS5wcm90b2J1Zi5TdHJ1Y3QSCgoCYnkYBSABKAki/QUKGlJlcG9ydENsaWVudEZhaWx1cmVSZXF1ZXN0ElMKBGtpbmQYASABKA4yHS5jYWRwb2xlLnYxLkNsaWVudEZhaWx1cmVLaW5kQiaKshkiGh5DTElFTlRfRkFJTFVSRV9LSU5EX1dBU01fUEFOSUMwARI+CgdtZXNzYWdlGAIgASgJQi2KshkpGiV0aW1lIG5vdCBpbXBsZW1lbnRlZCBvbiB0aGlzIHBsYXRmb3JtMAESQgoIbG9jYXRpb24YAyABKAlCMIqyGSwaKnNyYy9ydXN0L2NhZHBvbGUtY29yZS9zcmMvYm9vbGVhbi5yczo0MTI6ORINCgVzdGFjaxgEIAEoCRJFCgdzdXJmYWNlGAUgASgOMhkuY2FkcG9sZS52MS5DbGllbnRTdXJmYWNlQhmKshkVGhNDTElFTlRfU1VSRkFDRV9QQUdFEiEKCW9wZXJhdGlvbhgGIAEoCUIOirIZChoIZXZhbHVhdGUSGQoIcmV2aXNpb24YByABKARCB4qyGQMaATgSWQoGcGFyYW1zGAggAygLMjIuY2FkcG9sZS52MS5SZXBvcnRDbGllbnRGYWlsdXJlUmVxdWVzdC5QYXJhbXNFbnRyeUIVirIZEQoFcGFyYW0aCHdhbGw9Mi40EjIKDGVuZ2luZV9idWlsZBgJIAEoCUIcirIZGBoWY2FkcG9sZS13YXNtIDAuMS4wIG9wdBIfCgtlbmdpbmVfa2luZBgKIAEoCUIKirIZBhoEd2FzbRItCg1zY3JpcHRfZGlnZXN0GAsgASgJQhaKshkSGhA5ZjJiMWM0ZDVlNmY3YThiEhEKCXVwdGltZV9tcxgMIAEoBBIOCgZyZXBlYXQYDSABKA0aRQoLUGFyYW1zRW50cnkSCwoDa2V5GAEgASgJEiUKBXZhbHVlGAIgASgLMhYuY2FkcG9sZS52MS5QYXJhbVZhbHVlOgI4ATopirIZJQojUmVwb3J0IGEgZnJvbnQtZW5kIGNyYXNoIG9yIGZhaWx1cmUihAEKG1JlcG9ydENsaWVudEZhaWx1cmVSZXNwb25zZRILCgNzZXEYASABKAQSEAoIYWNjZXB0ZWQYAiABKAgSDwoHcmVmdXNlZBgDIAEoCRIPCgdkcm9wcGVkGAQgASgEEhEKCXJlbWFpbmluZxgFIAEoDRIRCgl0cnVuY2F0ZWQYBiADKAkimgEKDFNvbHZlUmVxdWVzdBIuCghkb2N1bWVudBgBIAEoCzIULmNhZHBvbGUudjEuRG9jdW1lbnRCBoqyGQIgARIcCgd3YWl0X21zGAIgASgNQguKshkHGgU2MDAwMBINCgVyZXJ1bhgDIAEoCDotirIZKQonUnVuIGEgZG9jdW1lbnQncyBwZW5kaW5nIHNvbHZlcyBhcyBqb2JzIpIBCg1Tb2x2ZVJlc3BvbnNlEiIKBGpvYnMYASADKAsyFC5jYWRwb2xlLnYxLlNvbHZlSm9iEg8KB3BlbmRpbmcYAiABKA0SEwoLZXZhbHVhdGlvbnMYAyABKA0SKwoLZGlhZ25vc3RpY3MYBCADKAsyFi5jYWRwb2xlLnYxLkRpYWdub3N0aWMSCgoCb2sYBSABKAgi2gIKCFNvbHZlSm9iEgoKAmlkGAEgASgJEg0KBXN0YXRlGAIgASgJEg4KBm9yaWdpbhgDIAEoCRIOCgZzdGF0dXMYBCABKAkSFgoJb2JqZWN0aXZlGAUgASgBSACIAQESEAoDZ2FwGAYgASgBSAGIAQESDQoFbm9kZXMYByABKAQSEQoJZWxhcHNlZF9zGAggASgBEhQKDHRpbWVfbGltaXRfcxgJIAEoARIMCgRrZXB0GAogASgIEg8KB21lc3NhZ2UYCyABKAkSDQoFb3duZXIYDCABKAkSCwoDcGlkGA0gASgNEhEKCXF1ZXVlZF9hdBgOIAEoCRITCgtmaW5pc2hlZF9hdBgPIAEoCRIRCgl2YXJpYWJsZXMYECABKAQSEwoLY29uc3RyYWludHMYESABKAQSEAoIbm9uemVyb3MYEiABKARCDAoKX29iamVjdGl2ZUIGCgRfZ2FwIk0KClNvbHZlQ2FjaGUSDAoEcGF0aBgBIAEoCRIPCgdhbnN3ZXJzGAIgASgEEg0KBWJ5dGVzGAMgASgEEhEKCWNhcF9ieXRlcxgEIAEoBCJjCg9MaXN0Sm9ic1JlcXVlc3QSHwoDaWRzGAEgAygJQhKKshkOGgwzZjlhMmMxYjdlNDQSCwoDYWxsGAIgASgIOiKKshkeChxMaXN0IG9wdGltaXNhdGlvbiBzb2x2ZSBqb2JzIl0KEExpc3RKb2JzUmVzcG9uc2USIgoEam9icxgBIAMoCzIULmNhZHBvbGUudjEuU29sdmVKb2ISJQoFY2FjaGUYAiABKAsyFi5jYWRwb2xlLnYxLlNvbHZlQ2FjaGUiewoPV2FpdEpvYnNSZXF1ZXN0Eh8KA2lkcxgBIAMoCUISirIZDhoMM2Y5YTJjMWI3ZTQ0Eh8KCnRpbWVvdXRfbXMYAiABKA1CC4qyGQcaBTMwMDAwOiaKshkiCiBXYWl0IGZvciBvcHRpbWlzYXRpb24gc29sdmUgam9icyJEChBXYWl0Sm9ic1Jlc3BvbnNlEiIKBGpvYnMYASADKAsyFC5jYWRwb2xlLnYxLlNvbHZlSm9iEgwKBGRvbmUYAiABKAgidQoRQ2FuY2VsSm9ic1JlcXVlc3QSHwoDaWRzGAEgAygJQhKKshkOGgwzZjlhMmMxYjdlNDQSCwoDYWxsGAIgASgIEg4KBmZvcmdldBgDIAEoCDoiirIZHgocU3RvcCBvcHRpbWlzYXRpb24gc29sdmUgam9icyI4ChJDYW5jZWxKb2JzUmVzcG9uc2USIgoEam9icxgBIAMoCzIULmNhZHBvbGUudjEuU29sdmVKb2IqgAEKCVJlZlN0YXR1cxIaChZSRUZfU1RBVFVTX1VOU1BFQ0lGSUVEEAASEQoNUkVGX1NUQVRVU19PSxABEhQKEFJFRl9TVEFUVVNfTU9WRUQQAhIUChBSRUZfU1RBVFVTX1NUQUxFEAMSGAoUUkVGX1NUQVRVU19BTUJJR1VPVVMQBCpbCgtSZXBseUZpbmlzaBIcChhSRVBMWV9GSU5JU0hfVU5TUEVDSUZJRUQQABIVChFSRVBMWV9GSU5JU0hfRE9ORRABEhcKE1JFUExZX0ZJTklTSF9GQUlMRUQQAir3AQoRQ2xpZW50RmFpbHVyZUtpbmQSIwofQ0xJRU5UX0ZBSUxVUkVfS0lORF9VTlNQRUNJRklFRBAAEiIKHkNMSUVOVF9GQUlMVVJFX0tJTkRfV0FTTV9QQU5JQxABEiEKHUNMSUVOVF9GQUlMVVJFX0tJTkRfRVhDRVBUSU9OEAISIQodQ0xJRU5UX0ZBSUxVUkVfS0lORF9SRUpFQ1RJT04QAxInCiNDTElFTlRfRkFJTFVSRV9LSU5EX09QRVJBVElPTl9FUlJPUhAEEioKJkNMSUVOVF9GQUlMVVJFX0tJTkRfRU5HSU5FX1VOQVZBSUxBQkxFEAUqoQEKDUNsaWVudFN1cmZhY2USHgoaQ0xJRU5UX1NVUkZBQ0VfVU5TUEVDSUZJRUQQABIXChNDTElFTlRfU1VSRkFDRV9QQUdFEAESGQoVQ0xJRU5UX1NVUkZBQ0VfV09SS0VSEAISIQodQ0xJRU5UX1NVUkZBQ0VfU0VSVklDRV9XT1JLRVIQAxIZChVDTElFTlRfU1VSRkFDRV9OQVRJVkUQBDL1GQoKQ2FkcG9sZU9wcxJdCghFdmFsdWF0ZRIbLmNhZHBvbGUudjEuRXZhbHVhdGVSZXF1ZXN0GhwuY2FkcG9sZS52MS5FdmFsdWF0ZVJlc3BvbnNlIhaKshkSCgRldmFsEAEqCGV2YWx1YXRlElQKBlJlbmRlchIZLmNhZHBvbGUudjEuUmVuZGVyUmVxdWVzdBoaLmNhZHBvbGUudjEuUmVuZGVyUmVzcG9uc2UiE4qyGQ8KBnJlbmRlchABIgNwbmcSUAoFU3dlZXASGC5jYWRwb2xlLnYxLlN3ZWVwUmVxdWVzdBoZLmNhZHBvbGUudjEuU3dlZXBSZXNwb25zZSISirIZDgoFc3dlZXAQASIDcG5nElMKB0FuaW1hdGUSGi5jYWRwb2xlLnYxLkFuaW1hdGVSZXF1ZXN0GhsuY2FkcG9sZS52MS5BbmltYXRlUmVzcG9uc2UiD4qyGQsKB2FuaW1hdGUQARJcCgZFeHBvcnQSGS5jYWRwb2xlLnYxLkV4cG9ydFJlcXVlc3QaGi5jYWRwb2xlLnYxLkV4cG9ydFJlc3BvbnNlIhuKshkXCgZleHBvcnQQASILcGxhdGVfc2hlZXQSZwoKUHJpbnRDaGVjaxIdLmNhZHBvbGUudjEuUHJpbnRDaGVja1JlcXVlc3QaHi5jYWRwb2xlLnYxLlByaW50Q2hlY2tSZXNwb25zZSIairIZFgoFY2hlY2sQASoLcHJpbnRfY2hlY2sSWAoFUGxhdGUSGC5jYWRwb2xlLnYxLlBsYXRlUmVxdWVzdBoZLmNhZHBvbGUudjEuUGxhdGVSZXNwb25zZSIairIZFgoFcGxhdGUQASILcGxhdGVfc2hlZXQSUwoHSW5zcGVjdBIaLmNhZHBvbGUudjEuSW5zcGVjdFJlcXVlc3QaGy5jYWRwb2xlLnYxLkluc3BlY3RSZXNwb25zZSIPirIZCwoHaW5zcGVjdBABElkKB0RyYXdpbmcSGi5jYWRwb2xlLnYxLkRyYXdpbmdSZXF1ZXN0GhsuY2FkcG9sZS52MS5EcmF3aW5nUmVzcG9uc2UiFYqyGREKB2RyYXdpbmcQASoEZHJhdxJTCghSdW5UZXN0cxIbLmNhZHBvbGUudjEuUnVuVGVzdHNSZXF1ZXN0GhwuY2FkcG9sZS52MS5SdW5UZXN0c1Jlc3BvbnNlIgyKshkICgR0ZXN0EAESWQoGQXBpRG9jEhkuY2FkcG9sZS52MS5BcGlEb2NSZXF1ZXN0GhouY2FkcG9sZS52MS5BcGlEb2NSZXNwb25zZSIYirIZFAoDZG9jEAEqC2dldF9hcGlfZG9jEmAKCkdldFNlc3Npb24SHS5jYWRwb2xlLnYxLkdldFNlc3Npb25SZXF1ZXN0GhMuY2FkcG9sZS52MS5TZXNzaW9uIh6KshkaCgdzZXNzaW9uEAEYASoLZ2V0X3Nlc3Npb24SdQoMR2V0U2VsZWN0aW9uEh8uY2FkcG9sZS52MS5HZXRTZWxlY3Rpb25SZXF1ZXN0GiAuY2FkcG9sZS52MS5HZXRTZWxlY3Rpb25SZXNwb25zZSIiirIZHgoJc2VsZWN0aW9uEAEYASoNZ2V0X3NlbGVjdGlvbhJoCglTZXRTY3JpcHQSHC5jYWRwb2xlLnYxLlNldFNjcmlwdFJlcXVlc3QaHS5jYWRwb2xlLnYxLlNldFNjcmlwdFJlc3BvbnNlIh6KshkaCgpzZXQtc2NyaXB0GAEqCnNldF9zY3JpcHQSaAoJU2V0UGFyYW1zEhwuY2FkcG9sZS52MS5TZXRQYXJhbXNSZXF1ZXN0Gh0uY2FkcG9sZS52MS5TZXRQYXJhbXNSZXNwb25zZSIeirIZGgoKc2V0LXBhcmFtcxgBKgpzZXRfcGFyYW1zEncKDFNldFNlbGVjdGlvbhIfLmNhZHBvbGUudjEuU2V0U2VsZWN0aW9uUmVxdWVzdBogLmNhZHBvbGUudjEuU2V0U2VsZWN0aW9uUmVzcG9uc2UiJIqyGSAKDXNldC1zZWxlY3Rpb24YASoNc2V0X3NlbGVjdGlvbhJQCgVSZXBseRIYLmNhZHBvbGUudjEuUmVwbHlSZXF1ZXN0GhkuY2FkcG9sZS52MS5SZXBseVJlc3BvbnNlIhKKshkOCgVyZXBseRgBKgNtc2cSdAoLU3RyZWFtUmVwbHkSHi5jYWRwb2xlLnYxLlN0cmVhbVJlcGx5UmVxdWVzdBofLmNhZHBvbGUudjEuU3RyZWFtUmVwbHlSZXNwb25zZSIkirIZIAoMc3RyZWFtLXJlcGx5GAEqDHN0cmVhbV9yZXBseTABEnQKC1Bvc3RNZXNzYWdlEh4uY2FkcG9sZS52MS5Qb3N0TWVzc2FnZVJlcXVlc3QaHy5jYWRwb2xlLnYxLlBvc3RNZXNzYWdlUmVzcG9uc2UiJIqyGSAKDHBvc3QtbWVzc2FnZRgBKgxwb3N0X21lc3NhZ2UwARJHCgRTYXZlEhcuY2FkcG9sZS52MS5TYXZlUmVxdWVzdBoYLmNhZHBvbGUudjEuU2F2ZVJlc3BvbnNlIgyKshkICgRzYXZlGAESaQoKV2FpdEV2ZW50cxIdLmNhZHBvbGUudjEuV2FpdEV2ZW50c1JlcXVlc3QaHi5jYWRwb2xlLnYxLldhaXRFdmVudHNSZXNwb25zZSIcirIZGAoGbGlzdGVuEAEYASoKd2FpdF9ldmVudBJtCgpFeHBsYWluUmVmEh0uY2FkcG9sZS52MS5FeHBsYWluUmVmUmVxdWVzdBoeLmNhZHBvbGUudjEuRXhwbGFpblJlZlJlc3BvbnNlIiCKshkcCgtleHBsYWluLXJlZhABKgtleHBsYWluX3JlZhJtCgpFZGl0U2NyaXB0Eh0uY2FkcG9sZS52MS5FZGl0U2NyaXB0UmVxdWVzdBoeLmNhZHBvbGUudjEuRWRpdFNjcmlwdFJlc3BvbnNlIiCKshkcCgtlZGl0LXNjcmlwdBgBKgtlZGl0X3NjcmlwdBJ/Cg1HZXRBdHRhY2htZW50EiAuY2FkcG9sZS52MS5HZXRBdHRhY2htZW50UmVxdWVzdBohLmNhZHBvbGUudjEuR2V0QXR0YWNobWVudFJlc3BvbnNlIimKshklCgphdHRhY2htZW50EAEYASIDcG5nKg5nZXRfYXR0YWNobWVudBJDCgNBY2sSFi5jYWRwb2xlLnYxLkFja1JlcXVlc3QaFy5jYWRwb2xlLnYxLkFja1Jlc3BvbnNlIguKshkHCgNhY2sYARJtCgpMaXN0QXNzZXRzEh0uY2FkcG9sZS52MS5MaXN0QXNzZXRzUmVxdWVzdBoeLmNhZHBvbGUudjEuTGlzdEFzc2V0c1Jlc3BvbnNlIiCKshkcCgthc3NldHMtbGlzdBABKgtsaXN0X2Fzc2V0cxJkCghBZGRBc3NldBIbLmNhZHBvbGUudjEuQWRkQXNzZXRSZXF1ZXN0GhwuY2FkcG9sZS52MS5BZGRBc3NldFJlc3BvbnNlIh2KshkZCgphc3NldHMtYWRkGAEqCWFkZF9hc3NldBJkCghHZXRBc3NldBIbLmNhZHBvbGUudjEuR2V0QXNzZXRSZXF1ZXN0GhwuY2FkcG9sZS52MS5HZXRBc3NldFJlc3BvbnNlIh2KshkZCgphc3NldHMtZ2V0EAEqCWdldF9hc3NldBKVAQoTUmVwb3J0Q2xpZW50RmFpbHVyZRImLmNhZHBvbGUudjEuUmVwb3J0Q2xpZW50RmFpbHVyZVJlcXVlc3QaJy5jYWRwb2xlLnYxLlJlcG9ydENsaWVudEZhaWx1cmVSZXNwb25zZSItirIZKQoOcmVwb3J0LWZhaWx1cmUYASoVcmVwb3J0X2NsaWVudF9mYWlsdXJlEkkKBVNvbHZlEhguY2FkcG9sZS52MS5Tb2x2ZVJlcXVlc3QaGS5jYWRwb2xlLnYxLlNvbHZlUmVzcG9uc2UiC4qyGQcKBXNvbHZlEl4KCExpc3RKb2JzEhsuY2FkcG9sZS52MS5MaXN0Sm9ic1JlcXVlc3QaHC5jYWRwb2xlLnYxLkxpc3RKb2JzUmVzcG9uc2UiF4qyGRMKBGpvYnMQASoJbGlzdF9qb2JzEmMKCFdhaXRKb2JzEhsuY2FkcG9sZS52MS5XYWl0Sm9ic1JlcXVlc3QaHC5jYWRwb2xlLnYxLldhaXRKb2JzUmVzcG9uc2UiHIqyGRgKCWpvYnMtd2FpdBABKgl3YWl0X2pvYnMSawoKQ2FuY2VsSm9icxIdLmNhZHBvbGUudjEuQ2FuY2VsSm9ic1JlcXVlc3QaHi5jYWRwb2xlLnYxLkNhbmNlbEpvYnNSZXNwb25zZSIeirIZGgoLam9icy1jYW5jZWwqC2NhbmNlbF9qb2JzYgZwcm90bzM",[zr,Xg,Uc,Qi,Dc,Yg]);var Kg=Zg(NE,0);var Pc=class extends Error{status;code;constructor(e,t,i="unknown"){super(t),this.status=e,this.code=i,this.name="ApiError"}};function IE(n){switch(n){case 400:return"invalid_argument";case 401:return"unauthenticated";case 403:return"permission_denied";case 404:return"unimplemented";case 409:return"aborted";case 412:return"failed_precondition";case 413:case 429:return"resource_exhausted";case 499:return"canceled";case 501:return"unimplemented";case 503:return"unavailable";case 504:return"deadline_exceeded";default:return n>=500?"internal":"unknown"}}var yd="application/proto";function BE(n){return n?n instanceof AbortSignal?{signal:n}:n:{}}var Gc=class{options;constructor(e){this.options=e}async unary(e,t,i,s){let{signal:r,keepalive:o}=BE(s),a=`${this.options.baseUrl}/${e.typeName}/${t.name}`,l=gd(t.input,Bs(t.input,i)),c={method:"POST",headers:{"content-type":yd,accept:yd,"connect-protocol-version":"1",...this.options.headers?.()??{}},cache:"no-store",body:l};r&&(c.signal=r),o&&(c.keepalive=!0);let u=await fetch(a,c);if(!u.ok)throw await this.failure(u,`${e.typeName}/${t.name}`);return Ma(t.output,new Uint8Array(await u.arrayBuffer()))}async failure(e,t){e.status===401&&this.options.onUnauthorized?.();let i=IE(e.status),s="";try{let r=await e.json();typeof r.code=="string"&&(i=r.code),typeof r.message=="string"&&(s=r.message)}catch{}return new Pc(e.status,`${t}: ${i}${s?`: ${s.slice(0,300)}`:""}`,i)}};function $g(n,e){let t={};for(let i of n.methods)t[i.localName]=(s,r)=>e.unary(n,i,s,r);return t}function Nt(n){return n!==null&&typeof n=="object"&&!Array.isArray(n)?n:{}}function an(n){return Array.isArray(n)?n:[]}function et(n,e=""){return typeof n=="string"?n:n==null?e:String(n)}function Ye(n,e=0){if(typeof n=="number")return Number.isFinite(n)?n:e;if(typeof n=="string"&&n.trim()!==""){let t=Number(n);return Number.isFinite(t)?t:e}return e}function _d(n){return n==null?null:Ye(n)}function Ra(n){return n===!0||n==="true"||n===1}function Vc(n){return an(n).map(e=>et(e))}function FE(n){let e={};for(let[t,i]of Object.entries(Nt(n)))e[t]=et(i);return e}function ei(n){if(Array.isArray(n))return{x:Ye(n[0]),y:Ye(n[1]),z:Ye(n[2])};let e=Nt(n);return{x:Ye(e.x),y:Ye(e.y),z:Ye(e.z)}}function LE(n){let e=Nt(n);return typeof e.x=="number"&&typeof e.y=="number"&&typeof e.z=="number"}function Jg(n){if(typeof n=="number"||typeof n=="boolean"||typeof n=="string")return n;if(Array.isArray(n))return ei(n);let e=Nt(n);return"number"in e?Ye(e.number):"boolean"in e?Ra(e.boolean):"text"in e?et(e.text):"vector"in e?ei(e.vector):LE(e)?{x:e.x,y:e.y,z:e.z}:0}function UE(n){if(typeof n=="number")return n>=3?"error":n===2?"warning":"info";let e=et(n).toLowerCase();return e.includes("err")?"error":e.includes("warn")?"warning":"info"}function qg(n){if(typeof n=="number")return["mm","mm","cm","m","in","ft"][n]??"mm";let e=et(n).toLowerCase().replace("length_unit_","");return e==="ft-in"||e==="ft_in"||e==="ftin"?"ft-in":e==="inch"?"in":e==="cm"||e==="m"||e==="in"||e==="ft"?e:"mm"}function Qg(n){if(n==null||typeof n!="object")return;let e=Nt(n);if(et(e.unit))return{unit:qg(e.unit),fraction:Ye(e.fraction,0)}}function bd(n){if(n==null)return null;let e=Nt(n),t={path:et(e.path),start_line:Ye(e.start_line),start_col:Ye(e.start_col),end_line:Ye(e.end_line),end_col:Ye(e.end_col)};return t.start_line===0&&t.end_line===0&&t.path===""?null:(t.end_line<t.start_line&&(t.end_line=t.start_line),t)}function jg(n){let e=Nt(n);return{severity:UE(e.severity),message:et(e.message),span:bd(e.span),feature_id:et(e.feature_id)}}function DE(n){let e=Nt(n);return{name:et(e.name),default_value:Jg(e.default_value),value:Jg(e.value),overridden:Ra(e.overridden),description:et(e.description),unit:et(e.unit),min:_d(e.min),max:_d(e.max),step:_d(e.step),choices:an(e.choices).map(t=>et(t)),span:bd(e.span)}}function PE(n){let e=Nt(n);return{id:et(e.id),kind:et(e.kind),name:et(e.name),inputs:Vc(e.inputs),outputs:Vc(e.outputs),args:FE(e.args),span:bd(e.span),suppressed:Ra(e.suppressed),diagnostics:an(e.diagnostics).map(jg)}}function GE(n){let e=Nt(n);return{name:et(e.name),surface_kind:et(e.surface_kind),centroid:ei(e.centroid),normal:ei(e.normal),area:Ye(e.area),edges:Vc(e.edges),tri_start:Ye(e.tri_start),tri_count:Ye(e.tri_count)}}function VE(n){let e=Nt(n);return{name:et(e.name),curve_kind:et(e.curve_kind),start:ei(e.start),end:ei(e.end),length:Ye(e.length),faces:Vc(e.faces),polyline:an(e.polyline).map(ei)}}function OE(n){let e=Nt(n);return{name:et(e.name),position:ei(e.position)}}function Sd(n){if(n==null)return null;if(Array.isArray(n))return n.length<3?null:[Ye(n[0]),Ye(n[1]),Ye(n[2])];if(typeof n=="object"){let e=Nt(n);return!("r"in e)&&!("g"in e)&&!("b"in e)?null:[Ye(e.r),Ye(e.g),Ye(e.b)]}return null}function kE(n){if(n==null)return null;let e=Nt(n),t=e.base_color,i=Ye(e.roughness),s=Ye(e.metallic),r=e.opacity;return{color:Sd(t),roughness:i>0?i:null,metallic:s>0?s:null,opacity:r==null?null:Ye(r)}}function zE(n,e){let t=Nt(n),i=Nt(t.topology),s=Nt(t.bbox);return{id:et(t.id,`body_${e}`),feature_id:et(t.feature_id),name:et(t.name),bbox:{min:ei(s.min),max:ei(s.max)},topology:{faces:an(i.faces).map(GE),edges:an(i.edges).map(VE),vertices:an(i.vertices).map(OE)},mesh_index:Ye(t.mesh_index,e),visible:t.visible===void 0?!0:Ra(t.visible),material:kE(t.material),part_body:et(t.part_body),transform:ex(t.transform),instance:et(t.instance),layer:et(t.layer)}}function WE(n,e){let t=Nt(n),i=Ye(t.opacity,1);return{name:et(t.name),doc:et(t.doc),visible:t.visible===void 0?!0:Ra(t.visible),opacity:Math.max(0,Math.min(1,Number.isFinite(i)?i:1)),color:Sd(t.color),order:Math.max(0,Math.round(Ye(t.order,e)))}}function ex(n){if(n==null)return null;let e=Nt(n),t=an(e.translation),i=an(e.rotation);return t.length<3||i.length<4?null:{translation:{x:Ye(t[0]),y:Ye(t[1]),z:Ye(t[2])},rotation:{x:Ye(i[0]),y:Ye(i[1]),z:Ye(i[2]),w:Ye(i[3],1)}}}function HE(n){let e=Nt(n);return{body_id:et(e.body_id),object:et(e.object),transform:ex(e.transform)??{translation:{x:0,y:0,z:0},rotation:{x:0,y:0,z:0,w:1}},slot:Math.max(1,Math.round(Ye(e.slot,1))),color:Sd(e.color)??[.75,.75,.75],filament:et(e.filament)}}function YE(n,e){let t=Nt(n);return{index:Math.max(1,Math.round(Ye(t.index,e+1))),name:et(t.name),path:et(t.path),bed:{x:Ye(t.bed_x,256),y:Ye(t.bed_y,256)},origin:ei(t.origin),bodies:an(t.bodies).map(HE)}}function vd(n){let e=Nt(n),t={params:an(e.params).map(DE),features:an(e.features).map(PE),bodies:an(e.bodies).map(zE),diagnostics:an(e.diagnostics).map(jg),eval_millis:Ye(e.eval_millis),plates:an(e.plates).map(YE),layers:an(e.layers).map(WE)},i=Qg(e.display_units??e.displayUnits);return i&&(t.display_units=i),t}var tx="cadpole.session.token",Td=null,Ed=!1;function ZE(){try{return sessionStorage.getItem(tx)}catch{return null}}function XE(){return Ed||(Td=ZE(),Ed=!0),Td}function nx(){let n=XE();return n?{authorization:`Bearer ${n}`}:{}}function ix(){Td=null,Ed=!0;try{sessionStorage.removeItem(tx)}catch{}}var eF=$g(Kg,new Gc({baseUrl:"/api",headers:nx,onUnauthorized:ix}));var $E=512<<20;async function sx(n,e=$E){let i=new Blob([n]).stream().pipeThrough(new DecompressionStream("deflate")).getReader(),s=[],r=0;for(;;){let{done:l,value:c}=await i.read();if(l)break;if(r+=c.byteLength,r>e)throw await i.cancel(),new Error(`this bake inflates past ${e>>20} MB`);s.push(c)}let o=new Uint8Array(r),a=0;for(let l of s)o.set(l,a),a+=l.byteLength;return o}var JE=[/^StackTrace\.js$/,/^TSLCore\.js$/,/^.*Node\.js$/,/^three\.webgpu.*\.js$/];function qE(n){let e=/(?:at\s+(.+?)\s+\()?(?:(.+?)@)?([^@\s()]+):(\d+):(\d+)/;return n.split(`
`).map(t=>{let i=t.match(e);if(!i)return null;let s=i[1]||i[2]||"",r=i[3].split("?")[0],o=parseInt(i[4],10),a=parseInt(i[5],10),l=r.split("/").pop();return{fn:s,file:l,line:o,column:a}}).filter(t=>t&&!JE.some(i=>i.test(t.file)))}var Gn=class{constructor(e=null){this.isStackTrace=!0,this.stack=qE(e||new Error().stack)}getLocation(){if(this.stack.length===0)return"[Unknown location]";let e=this.stack[0],t=e.fn;return`${t?`"${t}()" at `:""}"${e.file}:${e.line}"`}getError(e){if(this.stack.length===0)return e;let t=this.stack.map(i=>{let s=`${i.file}:${i.line}:${i.column}`;return i.fn?`    at ${i.fn} (${s})`:`    at ${s}`}).join(`
`);return`${e}
${t}`}};function Xf(n,e=0){let t=3735928559^e,i=1103547991^e;if(Array.isArray(n))for(let s=0,r;s<n.length;s++)r=n[s],t=Math.imul(t^r,2654435761),i=Math.imul(i^r,1597334677);else for(let s=0,r;s<n.length;s++)r=n.charCodeAt(s),t=Math.imul(t^r,2654435761),i=Math.imul(i^r,1597334677);return t=Math.imul(t^t>>>16,2246822507),t^=Math.imul(i^i>>>13,3266489909),i=Math.imul(i^i>>>16,2246822507),i^=Math.imul(t^t>>>13,3266489909),4294967296*(2097151&i)+(t>>>0)}var Ax=n=>Xf(n),QE=n=>Xf(n),Tx=(...n)=>Xf(n),jE=new Map([[1,"float"],[2,"vec2"],[3,"vec3"],[4,"vec4"],[9,"mat3"],[16,"mat4"]]);function eM(n){return jE.get(n)}function Xc(n){if(n==null)return null;let e=typeof n;return n.isNode===!0?"node":e==="number"?"float":e==="boolean"?"bool":e==="string"?"string":e==="function"?"shader":n.isVector2===!0?"vec2":n.isVector3===!0?"vec3":n.isVector4===!0?"vec4":n.isMatrix2===!0?"mat2":n.isMatrix3===!0?"mat3":n.isMatrix4===!0?"mat4":n.isColor===!0?"color":n instanceof ArrayBuffer?"ArrayBuffer":null}function tM(n){if(n.isDepthTexture===!0)return"float";let e=n.format,t;e===Do||e===Rr||e===Un||e===fi||e===Lo?t=1:e===Qn||e===wr?t=2:e===Uo||e===Mh?t=3:t=4;let i;if(n.type===bn?i="uint":n.type===Ts?i="int":i="float",t===1)return i;let s=eM(t);return i!=="float"&&(s=i[0]+s),s}function Kf(n,...e){let t=n?n.slice(-4):void 0;return e.length===1&&(t==="vec2"?e=[e[0],e[0]]:t==="vec3"?e=[e[0],e[0],e[0]]:t==="vec4"&&(e=[e[0],e[0],e[0],e[0]])),n==="color"?new He(...e):t==="vec2"?new ze(...e):t==="vec3"?new L(...e):t==="vec4"?new dt(...e):t==="mat2"?new Co(...e):t==="mat3"?new Ge(...e):t==="mat4"?new ct(...e):n==="bool"?e[0]||!1:n==="float"||n==="int"||n==="uint"?e[0]||0:n==="string"?e[0]||"":n==="ArrayBuffer"?iM(e[0]):null}function nM(n){let e="",t=new Uint8Array(n);for(let i=0;i<t.length;i++)e+=String.fromCharCode(t[i]);return btoa(e)}function iM(n){return Uint8Array.from(atob(n),e=>e.charCodeAt(0)).buffer}function Ex(n){return n[0]!==void 0&&n[0]!==null&&(n[0].isNode||Object.getPrototypeOf(n[0])!==Object.prototype)}var wa={VERTEX:"vertex",FRAGMENT:"fragment"},ot={NONE:"none",FRAME:"frame",RENDER:"render",OBJECT:"object"};var Md={READ_ONLY:"readOnly",WRITE_ONLY:"writeOnly",READ_WRITE:"readWrite"},sM=["fragment","vertex"];var lF=[...sM,"compute"],Ha=["x","y","z","w"],rM={analyze:"setup",generate:"analyze"},oM=0,De=class n extends wn{static get type(){return"Node"}constructor(e=null){super(),this.nodeType=e,this.updateType=ot.NONE,this.updateBeforeType=ot.NONE,this.updateAfterType=ot.NONE,this.version=0,this.name="",this.global=!1,this.parents=!1,this.isNode=!0,this._beforeNodes=null,this._cacheKey=null,this._uuid=null,this._cacheKeyVersion=0,this.id=oM++,this.stackTrace=null,n.captureStackTrace===!0&&(this.stackTrace=new Gn)}set needsUpdate(e){e===!0&&this.version++}get uuid(){return this._uuid===null&&(this._uuid=Lh.generateUUID()),this._uuid}get type(){return this.constructor.type}onUpdate(e,t){return this.updateType=t,this.update=e.bind(this),this}onFrameUpdate(e){return this.onUpdate(e,ot.FRAME)}onRenderUpdate(e){return this.onUpdate(e,ot.RENDER)}onObjectUpdate(e){return this.onUpdate(e,ot.OBJECT)}onReference(e){return this.updateReference=e.bind(this),this}updateReference(){return this}isGlobal(){return this.global}*getChildren(){for(let{childNode:e}of this._getChildren())yield e}dispose(){this.dispatchEvent({type:"dispose"})}traverse(e){e(this);for(let t of this.getChildren())t.traverse(e)}_getChildren(e=new Set){let t=[];e.add(this);for(let i of Object.getOwnPropertyNames(this)){let s=this[i];if(!(i.startsWith("_")===!0||e.has(s))){if(Array.isArray(s)===!0)for(let r=0;r<s.length;r++){let o=s[r];o&&o.isNode===!0&&t.push({property:i,index:r,childNode:o})}else if(s&&s.isNode===!0)t.push({property:i,childNode:s});else if(s&&Object.getPrototypeOf(s)===Object.prototype)for(let r in s){if(r.startsWith("_")===!0)continue;let o=s[r];o&&o.isNode===!0&&t.push({property:i,index:r,childNode:o})}}}return t}getCacheKey(e=!1,t=null){if(e=e||this.version!==this._cacheKeyVersion,e===!0||this._cacheKey===null){t===null&&(t=new Set);let i=[];for(let{property:s,childNode:r}of this._getChildren(t))i.push(Ax(s.slice(0,-4)),r.getCacheKey(e,t));this._cacheKey=Tx(QE(i),this.customCacheKey()),this._cacheKeyVersion=this.version}return this._cacheKey}customCacheKey(){return this.id}getScope(){return this}getHash(){return String(this.id)}getUpdateType(){return this.updateType}getUpdateBeforeType(){return this.updateBeforeType}getUpdateAfterType(){return this.updateAfterType}getElementType(e){let t=this.getNodeType(e);return e.getElementType(t)}getMemberType(){return"void"}getNodeType(e,t=null){let i=e.getDataFromNode(this),s;return t!==null?(i.typeFromOutput=i.typeFromOutput||{},s=i.typeFromOutput[t],s===void 0&&(s=this.generateNodeType(e,t),i.typeFromOutput[t]=s)):(s=i.type,s===void 0&&(s=this.generateNodeType(e),i.type=s)),s}generateNodeType(e,t=null){let i=e.getNodeProperties(this);return i.outputNode?i.outputNode.getNodeType(e,t):this.nodeType}getShared(e){let t=this.getHash(e),i=e.getNodeFromHash(t),s=null;if(i&&i!==this)s=i;else if(e.context.overrideNodes){let r=e.context.overrideNodes.get(this);if(r){let o=e.getDataFromNode(this);o.isOverwritten!==!0?(o.isOverwritten=!0,s=r(e).overrideNode(this,null),o.sharedNode=s):s=o.sharedNode}}return s||this}getArrayCount(){return null}setup(e){let t=e.getNodeProperties(this),i=0;for(let s of this.getChildren())t["node"+i++]=s;return t.outputNode||null}analyze(e,t=null){let i=e.increaseUsage(this);if(this.parents===!0){let s=e.getDataFromNode(this,"any");s.stages=s.stages||{},s.stages[e.shaderStage]=s.stages[e.shaderStage]||[],s.stages[e.shaderStage].push(t)}if(i===1){let s=e.getNodeProperties(this);for(let r of Object.values(s))r&&r.isNode===!0&&r.build(e,this)}}generate(e,t){let{outputNode:i}=e.getNodeProperties(this);if(i&&i.isNode===!0)return i.build(e,t)}updateBefore(){pe("Abstract function.")}updateAfter(){pe("Abstract function.")}update(){pe("Abstract function.")}before(e){return this._beforeNodes===null&&(this._beforeNodes=[]),this._beforeNodes.push(e),this}build(e,t=null){let i=this.getShared(e);if(this!==i)return i.build(e,t);if(this._beforeNodes!==null){let l=this._beforeNodes;this._beforeNodes=null;for(let c of l)c.build(e,t);this._beforeNodes=l}let s=e.getDataFromNode(this);s.buildStages=s.buildStages||{},s.buildStages[e.buildStage]=!0;let r=rM[e.buildStage];if(r&&s.buildStages[r]!==!0){let l=e.getBuildStage();e.setBuildStage(r),this.build(e),e.setBuildStage(l)}e.addChain(this);let o=null,a=e.getBuildStage();if(a==="setup"){e.addNode(this),this.updateReference(e);let l=e.getNodeProperties(this);if(l.initialized!==!0){l.initialized=!0,l.outputNode=this.setup(e)||l.outputNode||null;for(let c of Object.values(l))if(c&&c.isNode===!0){if(c.parents===!0){let u=e.getNodeProperties(c);u.parents=u.parents||[],u.parents.push(this)}c.build(e)}e.addSequentialNode(this)}o=l.outputNode}else if(a==="analyze")this.analyze(e,t);else if(a==="generate"){if(this.generate.length<2){let c=this.getNodeType(e),u=e.getDataFromNode(this);o=u.snippet,o===void 0?u.generated===void 0?(u.generated=!0,o=this.generate(e)||"",u.snippet=o):(pe("Node: Recursion detected.",this),o="/* Recursion detected. */"):u.flowCodes!==void 0&&e.context.nodeBlock!==void 0&&e.addFlowCodeHierarchy(this,e.context.nodeBlock),o=e.format(o,c,t)}else o=this.generate(e,t)||"";o===""&&t!==null&&t!=="void"&&t!=="OutputType"&&(_e(`TSL: Invalid generated code, expected a "${t}".`),o=e.generateConst(t))}return e.removeChain(this),o}getSerializeChildren(){return this._getChildren()}serialize(e){let t=this.getSerializeChildren(),i={};for(let{property:s,index:r,childNode:o}of t)r!==void 0?(i[s]===void 0&&(i[s]=Number.isInteger(r)?[]:{}),i[s][r]=o.toJSON(e.meta).uuid):i[s]=o.toJSON(e.meta).uuid;Object.keys(i).length>0&&(e.inputNodes=i)}deserialize(e){if(e.inputNodes!==void 0){let t=e.meta.nodes;for(let i in e.inputNodes)if(Array.isArray(e.inputNodes[i])){let s=[];for(let r of e.inputNodes[i])s.push(t[r]);this[i]=s}else if(typeof e.inputNodes[i]=="object"){let s={};for(let r in e.inputNodes[i]){let o=e.inputNodes[i][r];s[r]=t[o]}this[i]=s}else{let s=e.inputNodes[i];this[i]=t[s]}}}toJSON(e){let{uuid:t,type:i}=this,s=e===void 0||typeof e=="string";s&&(e={textures:{},images:{},nodes:{}});let r=e.nodes[t];r===void 0&&(r={uuid:t,type:i,meta:e,metadata:{version:4.7,type:"Node",generator:"Node.toJSON"}},s!==!0&&(e.nodes[r.uuid]=r),this.serialize(r),delete r.meta);function o(a){let l=[];for(let c in a){let u=a[c];delete u.metadata,l.push(u)}return l}if(s){let a=o(e.textures),l=o(e.images),c=o(e.nodes);a.length>0&&(r.textures=a),l.length>0&&(r.images=l),c.length>0&&(r.nodes=c)}return r}};De.captureStackTrace=!1;var Zr=class extends De{static get type(){return"ArrayElementNode"}constructor(e,t){super(),this.node=e,this.indexNode=t,this.isArrayElementNode=!0}generateNodeType(e){return this.node.getElementType(e)}getMemberType(e,t){return this.node.getMemberType(e,t)}generate(e){let t=this.indexNode.getNodeType(e),i=this.node.build(e),s=this.indexNode.build(e,!e.isVector(t)&&e.isInteger(t)?t:"uint");return`${i}[ ${s} ]`}},Kc=class extends De{static get type(){return"ConvertNode"}constructor(e,t){super(),this.node=e,this.convertTo=t}generateNodeType(e){let t=this.node.getNodeType(e),i=null;for(let s of this.convertTo.split("|"))(i===null||e.getTypeLength(t)===e.getTypeLength(s))&&(i=s);return i}serialize(e){super.serialize(e),e.convertTo=this.convertTo}deserialize(e){super.deserialize(e),this.convertTo=e.convertTo}generate(e,t){let i=this.node,s=this.getNodeType(e),r=i.build(e,s);return e.format(r,s,t)}},ln=class extends De{static get type(){return"TempNode"}constructor(e=null){super(e),this.isTempNode=!0}hasDependencies(e){return e.getDataFromNode(this).usageCount>1}build(e,t){if(e.getBuildStage()==="generate"){let s=e.getVectorType(this.getNodeType(e,t)),r=e.getDataFromNode(this);if(r.propertyName!==void 0)return e.format(r.propertyName,s,t);if(s!=="void"&&t!=="void"&&this.hasDependencies(e)){let o=super.build(e,s),a=e.getVarFromNode(this,null,s),l=e.getPropertyName(a);return e.addLineFlowCode(`${l} = ${o}`,this),r.snippet=o,r.propertyName=l,e.format(r.propertyName,s,t)}}return super.build(e,t)}},Dd=class extends ln{static get type(){return"JoinNode"}constructor(e=[],t=null){super(t),this.nodes=e}generateNodeType(e){return this.nodeType!==null?e.getVectorType(this.nodeType):e.getTypeFromLength(this.nodes.reduce((t,i)=>t+e.getTypeLength(i.getNodeType(e)),0))}generate(e,t){let i=this.getNodeType(e),s=e.getTypeLength(i),r=this.nodes,o=e.getComponentType(i),a=[],l=0;for(let u of r){if(l>=s){_e(`TSL: Length of parameters exceeds maximum length of function '${i}()' type.`,this.stackTrace);break}let d=u.getNodeType(e),h=e.getTypeLength(d),f;if(l+h>s&&(_e(`TSL: Length of '${i}()' data exceeds maximum length of output type.`,this.stackTrace),h=s-l,d=e.getTypeFromLength(h)),l+=h,f=u.build(e,d),e.getComponentType(d)!==o){let y=e.getTypeFromLength(h,o);f=e.format(f,d,y)}a.push(f)}let c=`${e.getType(i)}( ${a.join(", ")} )`;return e.format(c,i,t)}},aM=Ha.join(""),Pd=class extends De{static get type(){return"SplitNode"}constructor(e,t="x"){super(),this.node=e,this.components=t,this.isSplitNode=!0}getVectorLength(){let e=this.components.length;for(let t of this.components)e=Math.max(Ha.indexOf(t)+1,e);return e}getComponentType(e){return e.getComponentType(this.node.getNodeType(e))}generateNodeType(e){return e.getTypeFromLength(this.components.length,this.getComponentType(e))}getScope(){return this.node.getScope()}generate(e,t){let i=this.node,s=e.getTypeLength(i.getNodeType(e)),r=null;if(s>1){let o=null;this.getVectorLength()>=s&&(o=e.getTypeFromLength(this.getVectorLength(),this.getComponentType(e)));let l=i.build(e,o);this.components.length===s&&this.components===aM.slice(0,this.components.length)?r=e.format(l,o,t):r=e.format(`${l}.${this.components}`,this.getNodeType(e),t)}else r=i.build(e,t);return r}serialize(e){super.serialize(e),e.components=this.components}deserialize(e){super.deserialize(e),this.components=e.components}},Gd=class extends ln{static get type(){return"SetNode"}constructor(e,t,i){super(),this.sourceNode=e,this.components=t,this.targetNode=i}generateNodeType(e){return this.sourceNode.getNodeType(e)}generate(e){let{sourceNode:t,components:i,targetNode:s}=this,r=this.getNodeType(e),o=e.getComponentType(s.getNodeType(e)),a=e.getTypeFromLength(i.length,o),l=s.build(e,a),c=t.build(e,r),u=e.getTypeLength(r),d=[];for(let h=0;h<u;h++){let f=Ha[h];f===i[0]?(d.push(l),h+=i.length-1):d.push(c+"."+f)}return`${e.getType(r)}( ${d.join(", ")} )`}},Vd=class extends ln{static get type(){return"FlipNode"}constructor(e,t){super(),this.sourceNode=e,this.components=t}generateNodeType(e){return this.sourceNode.getNodeType(e)}generate(e){let{components:t,sourceNode:i}=this,s=this.getNodeType(e),r=i.build(e),o=e.getVarFromNode(this),a=e.getPropertyName(o);e.addLineFlowCode(a+" = "+r,this);let l=e.getTypeLength(s),c=[],u=0;for(let d=0;d<l;d++){let h=Ha[d];h===t[u]?(c.push("1.0 - "+(a+"."+h)),u++):c.push(a+"."+h)}return`${e.getType(s)}( ${c.join(", ")} )`}},Pa=class extends De{static get type(){return"InputNode"}constructor(e,t=null){super(t),this.isInputNode=!0,this.value=e,this.precision=null}generateNodeType(){return this.nodeType===null?Xc(this.value):this.nodeType}getInputType(e){return this.getNodeType(e)}setPrecision(e){return this.precision=e,this}serialize(e){super.serialize(e),e.value=this.value,this.value&&this.value.toArray&&(e.value=this.value.toArray()),e.valueType=Xc(this.value),e.nodeType=this.nodeType,e.valueType==="ArrayBuffer"&&(e.value=nM(e.value)),e.precision=this.precision}deserialize(e){super.deserialize(e),this.nodeType=e.nodeType,this.value=Array.isArray(e.value)?Kf(e.valueType,...e.value):e.value,this.precision=e.precision||null,this.value&&this.value.fromArray&&(this.value=this.value.fromArray(e.value))}generate(){pe("Abstract function.")}},rx=/float|u?int/,Vn=class extends Pa{static get type(){return"ConstNode"}constructor(e,t=null){super(e,t),this.isConstNode=!0}generateConst(e){return e.generateConst(this.getNodeType(e),this.value)}generate(e,t){let i=this.getNodeType(e);return rx.test(i)&&rx.test(t)?e.generateConst(t,this.value):e.format(this.generateConst(e),i,t)}},Od=class extends De{static get type(){return"MemberNode"}constructor(e,t){super(),this.structNode=e,this.property=t,this.isMemberNode=!0}hasMember(e){return this.structNode.isMemberNode&&this.structNode.hasMember(e)===!1?!1:this.structNode.getMemberType(e,this.property)!=="void"}generateNodeType(e){return this.hasMember(e)===!1?"float":this.structNode.getMemberType(e,this.property)}getMemberType(e,t){if(this.hasMember(e)===!1)return"float";let i=this.getNodeType(e);return e.getStructTypeNode(i).getMemberType(e,t)}generate(e){if(this.hasMember(e)===!1){pe(`TSL: Member "${this.property}" does not exist in struct.`,this.stackTrace);let i=this.getNodeType(e);return e.generateConst(i)}return this.structNode.build(e)+"."+this.property}},Ga=null,kd=new Map;function W(n,e){if(kd.has(n)){pe(`TSL: Redefinition of method chaining '${n}'.`);return}if(typeof e!="function")throw new Error(`THREE.TSL: Node element ${n} is not a function`);kd.set(n,e),n!=="assign"&&(De.prototype[n]=function(...t){return this.isStackNode?this.addToStack(e(...t)):e(this,...t)},De.prototype[n+"Assign"]=function(...t){return this.isStackNode?this.assign(t[0],e(...t)):this.assign(e(this,...t))})}var lM=n=>n.replace(/r|s/g,"x").replace(/g|t/g,"y").replace(/b|p/g,"z").replace(/a|q/g,"w"),ox=n=>lM(n).split("").sort().join("");De.prototype.assign=function(...n){if(this.isStackNode!==!0)return Ga!==null?Ga.assign(this,...n):_e("TSL: No stack defined for assign operation. Make sure the assign is inside a Fn().",new Gn),this;{let e=kd.get("assign");return this.addToStack(e(...n))}};De.prototype.toVarIntent=function(){return this};De.prototype.get=function(n){return new Od(this,n)};var Ua={};function Oc(n,e,t){Ua[n]=Ua[e]=Ua[t]={get(){this._cache=this._cache||{};let o=this._cache[n];return o===void 0&&(o=new Pd(this,n),this._cache[n]=o),o},set(o){this[n].assign(we(o))}};let i=n.toUpperCase(),s=e.toUpperCase(),r=t.toUpperCase();De.prototype["set"+i]=De.prototype["set"+s]=De.prototype["set"+r]=function(o){let a=ox(n);return new Gd(this,a,we(o))},De.prototype["flip"+i]=De.prototype["flip"+s]=De.prototype["flip"+r]=function(){let o=ox(n);return new Vd(this,o)}}var gi=["x","y","z","w"],xi=["r","g","b","a"],yi=["s","t","p","q"];for(let n=0;n<4;n++){let e=gi[n],t=xi[n],i=yi[n];Oc(e,t,i);for(let s=0;s<4;s++){e=gi[n]+gi[s],t=xi[n]+xi[s],i=yi[n]+yi[s],Oc(e,t,i);for(let r=0;r<4;r++){e=gi[n]+gi[s]+gi[r],t=xi[n]+xi[s]+xi[r],i=yi[n]+yi[s]+yi[r],Oc(e,t,i);for(let o=0;o<4;o++)e=gi[n]+gi[s]+gi[r]+gi[o],t=xi[n]+xi[s]+xi[r]+xi[o],i=yi[n]+yi[s]+yi[r]+yi[o],Oc(e,t,i)}}}for(let n=0;n<32;n++)Ua[n]={get(){this._cache=this._cache||{};let e=this._cache[n];return e===void 0&&(e=new Zr(this,new Vn(n,"uint")),this._cache[n]=e),e},set(e){this[n].assign(we(e))}};Object.defineProperties(De.prototype,Ua);var cM=function(n,e=null){let t=Xc(n);return t==="node"?n:e===null&&(t==="float"||t==="boolean")||t&&t!=="shader"&&t!=="string"?we(Hd(n,e)):t==="shader"?n.isFn?n:ke(n):n},uM=function(n,e=null){for(let t in n)n[t]=we(n[t],e);return n},hM=function(n,e=null){let t=n.length;for(let i=0;i<t;i++)n[i]=we(n[i],e);return n},Mx=function(n,e=null,t=null,i=null){function s(u){return i!==null?(u=we(Object.assign(u,i)),i.intent===!0&&(u=u.toVarIntent())):u=we(u),u}let r,o=e,a,l;function c(u){let d;return o?d=/[a-z]/i.test(o)?o+"()":o:d=n.type,a!==void 0&&u.length<a?(_e(`TSL: "${d}" parameter length is less than minimum required.`,new Gn),u.concat(new Array(a-u.length).fill(0))):l!==void 0&&u.length>l?(_e(`TSL: "${d}" parameter length exceeds limit.`,new Gn),u.slice(0,l)):u}return e===null?r=(...u)=>s(new n(...Yr(c(u)))):t!==null?(t=we(t),r=(...u)=>s(new n(e,...Yr(c(u)),t))):r=(...u)=>s(new n(e,...Yr(c(u)))),r.setParameterLength=(...u)=>(u.length===1?a=l=u[0]:u.length===2&&([a,l]=u),r),r.setName=u=>(o=u,r),r},dM=function(n,...e){return new n(...Yr(e))},zd=class extends De{constructor(e,t){super(),this.shaderNode=e,this.rawInputs=t,this.isShaderCallNodeInternal=!0}generateNodeType(e){return this.shaderNode.nodeType||this.getOutputNode(e).getNodeType(e)}getElementType(e){return this.getOutputNode(e).getElementType(e)}getMemberType(e,t){return this.getOutputNode(e).getMemberType(e,t)}call(e){let{shaderNode:t,rawInputs:i}=this,s=e.getNodeProperties(t),r=e.getClosestSubBuild(t.subBuilds)||"",o=r||"default";if(s[o])return s[o];let a=e.subBuildFn,l=e.fnCall;e.subBuildFn=r,e.fnCall=this;let c=null;if(t.layout){if(i){let h=t.layout.inputs;if(Ex(i)){let f=i;for(let g=0;g<h.length;g++){let y=f[g];y&&y.isNode&&y.build(e)}}else{let f=i[0];for(let g of h){let y=f[g.name];y&&y.isNode&&y.build(e)}}}let u=e.buildFunctionNode(t);e.addInclude(u);let d=i?fM(i):null;c=u.call(d)}else{let u=new Proxy(e,{get:(y,m,p)=>{let E;return Symbol.iterator===m?E=function*(){yield void 0}:E=Reflect.get(y,m,p),E}}),d=i?pM(i):null,h=Array.isArray(i)?i.length>0:i!==null,f=t.jsFunc,g=h||f.length>1?f(d,u):f(u);c=we(g)}return e.subBuildFn=a,e.fnCall=l,t.once&&(s[o]=c),c}setupOutput(e){return e.addStack(),e.stack.outputNode=this.call(e),e.removeStack()}getOutputNode(e){let t=e.getNodeProperties(this),i=e.getSubBuildOutput(this);return t[i]=t[i]||this.setupOutput(e),t[i].subBuild=e.getClosestSubBuild(this),t[i]}build(e,t=null){let i=null,s=e.getBuildStage(),r=e.getNodeProperties(this),o=e.getSubBuildOutput(this),a=this.getOutputNode(e),l=e.fnCall;if(e.fnCall=this,s==="setup"){let c=e.getSubBuildProperty("initialized",this);if(r[c]!==!0&&(r[c]=!0,r[o]=this.getOutputNode(e),r[o].build(e),this.shaderNode.subBuilds))for(let u of e.chaining){let d=e.getDataFromNode(u,"any");d.subBuilds=d.subBuilds||new Set;for(let h of this.shaderNode.subBuilds)d.subBuilds.add(h)}i=r[o]}else s==="analyze"?a.build(e,t):s==="generate"&&(i=a.build(e,t)||"");return e.fnCall=l,i}};function fM(n){let e;return Qf(n),Ex(n)?e=[...n]:e=n[0],e}function pM(n){let e=0;return Qf(n),new Proxy(n,{get:(t,i,s)=>{let r;if(i==="length")return r=n.length,r;if(Symbol.iterator===i)r=function*(){for(let o of n)yield we(o)};else{if(n.length>0)if(Object.getPrototypeOf(n[0])===Object.prototype){let o=n[0];o[i]===void 0?r=o[e++]:r=Reflect.get(o,i,s)}else n[0]instanceof De&&(n[i]===void 0?r=n[e++]:r=Reflect.get(n,i,s));else r=Reflect.get(t,i,s);r=we(r)}return r}})}var Wd=class extends De{constructor(e,t){super(t),this.jsFunc=e,this.layout=null,this.global=!0,this.once=!1}setLayout(e){return this.layout=e,this}getLayout(){return this.layout}call(e=null){return new zd(this,e)}setup(){return this.call()}},mM=[!1,!0],gM=[0,1,2,3],xM=[-1,-2],Rx=[.5,1.5,1/3,1e-6,1e6,Math.PI,Math.PI*2,1/Math.PI,2/Math.PI,1/(Math.PI*2),Math.PI/2],$f=new Map;for(let n of mM)$f.set(n,new Vn(n));var Jf=new Map;for(let n of gM)Jf.set(n,new Vn(n,"uint"));var qf=new Map([...Jf].map(n=>new Vn(n.value,"int")));for(let n of xM)qf.set(n,new Vn(n,"int"));var cu=new Map([...qf].map(n=>new Vn(n.value)));for(let n of Rx)cu.set(n,new Vn(n));for(let n of Rx)cu.set(-n,new Vn(-n));var uu={bool:$f,uint:Jf,ints:qf,float:cu},ax=new Map([...$f,...cu]),Hd=(n,e)=>ax.has(n)?ax.get(n):n.isNode===!0?n:new Vn(n,e),Vt=function(n,e=null){return(...t)=>{for(let s of t)if(s===void 0)return _e(`TSL: Invalid parameter for the type "${n}".`,new Gn),new Vn(0,n);if((t.length===0||!["bool","float","int","uint"].includes(n)&&t.every(s=>{let r=typeof s;return r!=="object"&&r!=="function"}))&&(t=[Kf(n,...t)]),t.length===1&&e!==null&&e.has(t[0]))return kc(e.get(t[0]));if(t.length===1){let s=Hd(t[0],n);return s.nodeType===n?kc(s):kc(new Kc(s,n))}let i=t.map(s=>Hd(s));return kc(new Dd(i,n))}};function yM(n){return n&&n.isNode&&n.traverse(e=>{e.isConstNode&&(n=e.value)}),!!n}var _M=n=>n!=null?n.nodeType||n.convertTo||(typeof n=="string"?n:null):null;function bM(n,e){return new Wd(n,e)}var we=(n,e=null)=>cM(n,e),kc=(n,e=null)=>we(n,e).toVarIntent(),Qf=(n,e=null)=>new uM(n,e),Yr=(n,e=null)=>new hM(n,e),qt=(n,e=null,t=null,i=null)=>new Mx(n,e,t,i),Ne=(n,...e)=>new dM(n,...e),ge=(n,e=null,t=null,i={})=>new Mx(n,e,t,{...i,intent:!0});var SM=0,Yd=class extends De{constructor(e,t=null){super();let i=null;t!==null&&(typeof t=="object"?i=t.return:(typeof t=="string"?i=t:_e("TSL: Invalid layout type.",new Gn),t=null)),this.shaderNode=new bM(e,i),t!==null&&this.setLayout(t),this.isFn=!0}setLayout(e){let t=this.shaderNode.nodeType;if(typeof e.inputs!="object"){let i={name:"fn"+SM++,type:t,inputs:[]};for(let s in e)s!=="return"&&i.inputs.push({name:s,type:e[s]});e=i}return this.shaderNode.setLayout(e),this}generateNodeType(e){return this.shaderNode.getNodeType(e)||"float"}call(...e){let t=this.shaderNode.call(e);return this.shaderNode.nodeType==="void"&&t.toStack(),t.toVarIntent()}once(e=null){return this.shaderNode.once=!0,this.shaderNode.subBuilds=e,this}generate(e){let t=this.getNodeType(e);return _e('TSL: "Fn()" was declared but not invoked. Try calling it like "Fn()( ...params )".',this.stackTrace),e.generateConst(t)}};function ke(n,e=null){let t=new Yd(n,e);return new Proxy(()=>{},{apply(i,s,r){return t.call(...r)},get(i,s,r){return Reflect.get(t,s,r)},set(i,s,r,o){return Reflect.set(t,s,r,o)}})}var Wr=(...n)=>Ga.If(...n);function vM(n){return Ga&&Ga.addToStack(n),n}W("toStack",vM);var AM=new Vt("color"),it=new Vt("float",uu.float),hu=new Vt("int",uu.ints),TM=new Vt("uint",uu.uint),du=new Vt("bool",uu.bool),Vs=new Vt("vec2"),EM=new Vt("ivec2"),MM=new Vt("uvec2"),RM=new Vt("bvec2"),wt=new Vt("vec3"),wM=new Vt("ivec3"),CM=new Vt("uvec3"),NM=new Vt("bvec3"),gt=new Vt("vec4"),IM=new Vt("ivec4"),BM=new Vt("uvec4"),FM=new Vt("bvec4"),wx=new Vt("mat2"),Os=new Vt("mat3"),Cx=new Vt("mat4");W("toColor",AM);W("toFloat",it);W("toInt",hu);W("toUint",TM);W("toBool",du);W("toVec2",Vs);W("toIVec2",EM);W("toUVec2",MM);W("toBVec2",RM);W("toVec3",wt);W("toIVec3",wM);W("toUVec3",CM);W("toBVec3",NM);W("toVec4",gt);W("toIVec4",IM);W("toUVec4",BM);W("toBVec4",FM);W("toMat2",wx);W("toMat3",Os);W("toMat4",Cx);var LM=qt(Zr).setParameterLength(2),UM=(n,e)=>new Kc(we(n),e);W("element",LM);W("convert",UM);var Ds=class extends De{static get type(){return"PropertyNode"}constructor(e,t=null,i=!1,s=null){super(e),this.name=t,this.varying=i,this.placeholderNode=we(s),this.isPropertyNode=!0,this.global=!0}getNodeType(e){let t=super.getNodeType(e);return t==="output"?e.getOutputType():t}customCacheKey(){return Ax(this.type+":"+(this.name||"")+":"+(this.varying?"1":"0"))}getHash(e){return this.name||super.getHash(e)}generate(e){let t;if(this.varying===!0)t=e.getVaryingFromNode(this,this.name),t.needsInterpolation=!0;else if(t=e.getVarFromNode(this,this.name),this.placeholderNode!==null&&e.hasWriteUsage(this)===!1){let i=this.placeholderNode.build(e,this.getNodeType(e));e.addLineFlowCode(`${e.getPropertyName(t)} = ${i}`,this)}return e.getPropertyName(t)}},ks=(n,e,t=null)=>new Ds(n,e,!1,t),fu=(n,e,t=null)=>new Ds(n,e,!0,t),lx=Ne(Ds,"vec4","DiffuseColor");var Rd=Ne(Ds,"float","dashSize"),cx=Ne(Ds,"float","gapSize");var $c=class extends De{static get type(){return"UniformGroupNode"}constructor(e,t=!1,i=1,s=null){super("string"),this.name=e,this.shared=t,this.order=i,this.updateType=s,this.isUniformGroup=!0}update(){this.needsUpdate=!0}serialize(e){super.serialize(e),e.name=this.name,e.version=this.version,e.shared=this.shared}deserialize(e){super.deserialize(e),this.name=e.name,this.version=e.version,this.shared=e.shared}},DM=(n,e=1,t=null)=>new $c(n,!1,e,t),jf=(n,e=0,t=null)=>new $c(n,!0,e,t),cF=jf("frame",0,ot.FRAME),Wt=jf("render",0,ot.RENDER),PM=DM("object",1,ot.OBJECT),Ps=class extends Pa{static get type(){return"UniformNode"}constructor(e,t=null){super(e,t),this.isUniformNode=!0,this.name="",this.groupNode=PM}setName(e){return this.name=e,this}label(e){return pe('TSL: "label()" has been deprecated. Use "setName()" instead.',new Gn),this.setName(e)}setGroup(e){return this.groupNode=e,this}getGroup(){return this.groupNode}getUniformHash(e){return this.getHash(e)}getSharedNode(e){let t=this.getUniformHash(e),i=e.getNodeFromHash(t);return i===void 0&&(e.setHashNode(this,t),i=this),i}onUpdate(e,t){return e=e.bind(this),super.onUpdate(i=>{let s=e(i,this);s!==void 0&&(this.value=s)},t)}getInputType(e){let t=super.getInputType(e);return t==="bool"&&(t="uint"),t}generate(e,t){let i=this.getNodeType(e),s=this.getSharedNode(e),r=s.getInputType(e),o=e.getUniformFromNode(s,r,e.shaderStage,this.name||e.context.nodeName),a=e.getPropertyName(o);e.context.nodeName!==void 0&&delete e.context.nodeName;let l=a;if(i==="bool"){let c=e.getDataFromNode(this),u=c.propertyName;if(u===void 0){let d=e.getVarFromNode(this,null,"bool");u=e.getPropertyName(d),c.propertyName=u,l=e.format(a,r,i),e.addLineFlowCode(`${u} = ${l}`,this)}l=u}return e.format(l,i,t)}},Ht=(n,e)=>{let t=_M(e||n);if(t===n&&(n=Kf(t)),n&&n.isNode===!0){let i=n.value;n.traverse(s=>{s.isConstNode===!0&&(i=s.value)}),n=i}return new Ps(n,t)},Jc=class extends ln{static get type(){return"ArrayNode"}constructor(e,t,i=null){super(e),this.count=t,this.values=i,this.isArrayNode=!0}getArrayCount(){return this.count}generateNodeType(e){return this.nodeType===null?this.values[0].getNodeType(e):this.nodeType}getElementType(e){return this.getNodeType(e)}getMemberType(e,t){return this.nodeType===null?this.values[0].getMemberType(e,t):super.getMemberType(e,t)}generate(e){let t=this.getNodeType(e);return e.generateArray(t,this.count,this.values)}},Zd=(...n)=>{let e;if(n.length===1){let t=n[0].map(i=>we(i));e=new Jc(null,t.length,t)}else{let t=n[0],i=n[1];e=new Jc(t,i)}return e};W("toArray",(n,e)=>Zd(Array(e).fill(n)));var Xd=class extends ln{static get type(){return"AssignNode"}constructor(e,t){super(),this.targetNode=e,this.sourceNode=t,this.isAssignNode=!0}hasDependencies(){return!1}generateNodeType(e,t){return t!=="void"?this.targetNode.getNodeType(e):"void"}needsSplitAssign(e){let{targetNode:t}=this;if(e.isAvailable("swizzleAssign")===!1&&t.isSplitNode&&t.components.length>1){let i=e.getTypeLength(t.node.getNodeType(e));return Ha.join("").slice(0,i)!==t.components}return!1}setup(e){let{targetNode:t,sourceNode:i}=this,s=t.getScope(),r=e.getDataFromNode(s);r.assign=!0;let o=e.getNodeProperties(this);o.sourceNode=i,o.targetNode=t.context({assign:!0})}generate(e,t){let{targetNode:i,sourceNode:s}=e.getNodeProperties(this),r=this.needsSplitAssign(e),o=i.build(e),a=i.getNodeType(e),l=s.build(e,a),c=s.getNodeType(e),u=e.getDataFromNode(this),d;if(u.initialized===!0)t!=="void"&&(d=o);else if(r){let h=e.getVarFromNode(this,null,a),f=e.getPropertyName(h);e.addLineFlowCode(`${f} = ${l}`,this);let g=i.node,m=g.node.context({assign:!0}).build(e);for(let p=0;p<g.components.length;p++){let E=g.components[p];e.addLineFlowCode(`${m}.${E} = ${f}[ ${p} ]`,this)}t!=="void"&&(d=o)}else d=`${o} = ${l}`,(t==="void"||c==="void")&&(e.addLineFlowCode(d,this),t!=="void"&&(d=o));return u.initialized=!0,e.format(d,a,t)}},GM=qt(Xd).setParameterLength(2);W("assign",GM);var Kd=class extends ln{static get type(){return"FunctionCallNode"}constructor(e=null,t={}){super(),this.functionNode=e,this.parameters=t}setParameters(e){return this.parameters=e,this}getParameters(){return this.parameters}generateNodeType(e){return this.functionNode.getNodeType(e)}getMemberType(e,t){return this.functionNode.getMemberType(e,t)}generate(e){let t=[],i=this.functionNode,s=i.getInputs(e),r=this.parameters,o=(l,c)=>{let u=c.type,d=u==="pointer",h;return d?h="&"+l.build(e):h=l.build(e,u),h};if(Array.isArray(r)){if(r.length>s.length)_e("TSL: The number of provided parameters exceeds the expected number of inputs in 'Fn()'."),r.length=s.length;else if(r.length<s.length)for(_e("TSL: The number of provided parameters is less than the expected number of inputs in 'Fn()'.");r.length<s.length;)r.push(it(0));for(let l=0;l<r.length;l++)t.push(o(r[l],s[l]))}else for(let l of s){let c=r[l.name];c!==void 0?t.push(o(c,l)):(_e(`TSL: Input '${l.name}' not found in 'Fn()'.`),t.push(o(it(0),l)))}return`${i.build(e,"property")}( ${t.join(", ")} )`}},VM=(n,...e)=>(e=e.length>1||e[0]&&e[0].isNode===!0?Yr(e):Qf(e[0]),new Kd(we(n),e));W("call",VM);var OM={"==":"equal","!=":"notEqual","<":"lessThan",">":"greaterThan","<=":"lessThanEqual",">=":"greaterThanEqual","%":"mod"},Ct=class n extends ln{static get type(){return"OperatorNode"}constructor(e,t,i,...s){if(super(),s.length>0){let r=new n(e,t,i);for(let o=0;o<s.length-1;o++)r=new n(e,r,s[o]);t=r,i=s[s.length-1]}this.op=e,this.aNode=t,this.bNode=i,this.isOperatorNode=!0}getOperatorMethod(e,t){return e.getMethod(OM[this.op],t)}generateNodeType(e,t=null){let i=this.op,s=this.aNode,r=this.bNode,o=s.getNodeType(e),a=r?r.getNodeType(e):null;if(o==="void"||a==="void")return t||"void";if(i==="%")return o;if(i==="~"||i==="&"||i==="|"||i==="^"||i===">>"||i==="<<")return e.getIntegerType(o);if(i==="&&"||i==="||"||i==="^^")return"bool";if(i==="!"){let l=e.getTypeLength(o);return l>1?`bvec${l}`:"bool"}else if(i==="=="||i==="!="||i==="<"||i===">"||i==="<="||i===">="){let l=Math.max(e.getTypeLength(o),e.getTypeLength(a));return l>1?`bvec${l}`:"bool"}else{if(e.isMatrix(o)){if(a==="float")return o;if(e.isVector(a))return e.getVectorFromMatrix(o);if(e.isMatrix(a))return o}else if(e.isMatrix(a)){if(o==="float")return a;if(e.isVector(o))return e.getVectorFromMatrix(a)}return e.getTypeLength(a)>e.getTypeLength(o)?a:o}}generate(e,t){let i=this.op,{aNode:s,bNode:r}=this,o=this.getNodeType(e,t),a=null,l=null;o!=="void"?(a=s.getNodeType(e),l=r?r.getNodeType(e):null,i==="<"||i===">"||i==="<="||i===">="||i==="=="||i==="!="?e.isVector(a)?l=a:e.isVector(l)?a=l:a!==l&&(a=l="float"):i===">>"||i==="<<"?(a=o,l=e.changeComponentType(l,"uint")):i==="%"?(a=o,l=e.isInteger(a)&&e.isInteger(l)?l:a):e.isMatrix(a)?l==="float"?l="float":e.isVector(l)?l=e.getVectorFromMatrix(a):e.isMatrix(l)||(a=l=o):e.isMatrix(l)?a==="float"?a="float":e.isVector(a)?a=e.getVectorFromMatrix(l):a=l=o:a=l=o):a=l=o;let c=s.build(e,a),u=r?r.build(e,l):null,d=e.getFunctionOperator(i);if(t!=="void"){let h=e.renderer.coordinateSystem===hn;if(i==="=="||i==="!="||i==="<"||i===">"||i==="<="||i===">=")return h?e.isVector(a)?e.format(`${this.getOperatorMethod(e,t)}( ${c}, ${u} )`,o,t):e.format(`( ${c} ${i} ${u} )`,o,t):e.format(`( ${c} ${i} ${u} )`,o,t);if(i==="%")return e.isInteger(l)?e.format(`( ${c} % ${u} )`,o,t):e.format(`${this.getOperatorMethod(e,o)}( ${c}, ${u} )`,o,t);if(i==="!")return h&&e.isVector(a)?e.format(`not( ${c} )`,t):e.format(`( ${i} ${c} )`,a,t);if(i==="~")return e.format(`( ${i} ${c} )`,a,t);if(d)return e.format(`${d}( ${c}, ${u} )`,o,t);if(e.isMatrix(a)&&l==="float")return e.format(`( ${u} ${i} ${c} )`,o,t);if(a==="float"&&e.isMatrix(l))return e.format(`${c} ${i} ${u}`,o,t);{let f=`( ${c} ${i} ${u} )`;return!h&&o==="bool"&&e.isVector(a)&&e.isVector(l)&&(f=`all${f}`),e.format(f,o,t)}}else if(a!=="void")return d?e.format(`${d}( ${c}, ${u} )`,o,t):e.isMatrix(a)&&l==="float"?e.format(`${u} ${i} ${c}`,o,t):e.format(`${c} ${i} ${u}`,o,t)}serialize(e){super.serialize(e),e.op=this.op}deserialize(e){super.deserialize(e),this.op=e.op}},kM=ge(Ct,"+").setParameterLength(2,1/0).setName("add"),$d=ge(Ct,"-").setParameterLength(2,1/0).setName("sub"),zs=ge(Ct,"*").setParameterLength(2,1/0).setName("mul"),Nx=ge(Ct,"/").setParameterLength(2,1/0).setName("div"),Ix=ge(Ct,"%").setParameterLength(2).setName("mod"),zM=ge(Ct,"==").setParameterLength(2).setName("equal"),WM=ge(Ct,"!=").setParameterLength(2).setName("notEqual"),HM=ge(Ct,"<").setParameterLength(2).setName("lessThan"),YM=ge(Ct,">").setParameterLength(2).setName("greaterThan"),ZM=ge(Ct,"<=").setParameterLength(2).setName("lessThanEqual"),XM=ge(Ct,">=").setParameterLength(2).setName("greaterThanEqual"),KM=ge(Ct,"&&").setParameterLength(2,1/0).setName("and"),$M=ge(Ct,"||").setParameterLength(2,1/0).setName("or"),JM=ge(Ct,"!").setParameterLength(1).setName("not"),qM=ge(Ct,"^^").setParameterLength(2).setName("xor"),QM=ge(Ct,"&").setParameterLength(2).setName("bitAnd"),jM=ge(Ct,"~").setParameterLength(1).setName("bitNot"),eR=ge(Ct,"|").setParameterLength(2).setName("bitOr"),tR=ge(Ct,"^").setParameterLength(2).setName("bitXor"),nR=ge(Ct,"<<").setParameterLength(2).setName("shiftLeft"),iR=ge(Ct,">>").setParameterLength(2).setName("shiftRight"),sR=ke(([n])=>(n.addAssign(1),n)),rR=ke(([n])=>(n.subAssign(1),n)),oR=ke(([n])=>{let e=hu(n).toConst();return n.addAssign(1),e}),aR=ke(([n])=>{let e=hu(n).toConst();return n.subAssign(1),e});W("add",kM);W("sub",$d);W("mul",zs);W("div",Nx);W("mod",Ix);W("equal",zM);W("notEqual",WM);W("lessThan",HM);W("greaterThan",YM);W("lessThanEqual",ZM);W("greaterThanEqual",XM);W("and",KM);W("or",$M);W("not",JM);W("xor",qM);W("bitAnd",QM);W("bitNot",jM);W("bitOr",eR);W("bitXor",tR);W("shiftLeft",nR);W("shiftRight",iR);W("incrementBefore",sR);W("decrementBefore",rR);W("increment",oR);W("decrement",aR);var P=class n extends ln{static get type(){return"MathNode"}constructor(e,t,i=null,s=null){if(super(),(e===n.MAX||e===n.MIN)&&arguments.length>3){let r=new n(e,t,i);for(let o=3;o<arguments.length-1;o++)r=new n(e,r,arguments[o]);t=r,i=arguments[arguments.length-1],s=null}this.method=e,this.aNode=t,this.bNode=i,this.cNode=s,this.isMathNode=!0}getInputType(e){let t=this.aNode.getNodeType(e),i=this.bNode?this.bNode.getNodeType(e):null,s=this.cNode?this.cNode.getNodeType(e):null,r=e.isMatrix(t)?0:e.getTypeLength(t),o=e.isMatrix(i)?0:e.getTypeLength(i),a=e.isMatrix(s)?0:e.getTypeLength(s);return r>o&&r>a?t:o>a?i:a>r?s:t}generateNodeType(e){let t=this.method;return t===n.LENGTH||t===n.DISTANCE||t===n.DOT||t===n.DETERMINANT?"float":t===n.CROSS?"vec3":t===n.ALL||t===n.ANY?"bool":t===n.EQUALS?e.changeComponentType(this.aNode.getNodeType(e),"bool"):this.getInputType(e)}setup(e){let{aNode:t,bNode:i,method:s}=this,r=null;if(s===n.ONE_MINUS)r=$d(1,t);else if(s===n.RECIPROCAL)r=Nx(1,t);else if(s===n.DIFFERENCE)r=ep($d(t,i));else if(s===n.TRANSFORM_DIRECTION){let o,a;e.isMatrix(t.getNodeType(e))?(o=t,a=i):(o=i,a=t),r=pu(zs(o,gt(wt(a),0)).xyz)}return r!==null?r:super.setup(e)}generate(e,t){if(e.getNodeProperties(this).outputNode)return super.generate(e,t);let s=this.method,r=this.getNodeType(e),o=this.getInputType(e),a=this.aNode,l=this.bNode,c=this.cNode,u=e.renderer.coordinateSystem;if(s===n.NEGATE)return e.format("( - "+a.build(e,o)+" )",r,t);{let d=[];return s===n.CROSS?d.push(a.build(e,r),l.build(e,r)):u===hn&&s===n.STEP?d.push(a.build(e,e.getTypeLength(a.getNodeType(e))===1?"float":o),l.build(e,o)):u===hn&&(s===n.MIN||s===n.MAX)?d.push(a.build(e,o),l.build(e,e.getTypeLength(l.getNodeType(e))===1?e.getComponentType(o):o)):s===n.REFRACT?d.push(a.build(e,o),l.build(e,o),c.build(e,"float")):s===n.MIX?d.push(a.build(e,o),l.build(e,o),c.build(e,e.getTypeLength(c.getNodeType(e))===1?"float":o)):(u===li&&s===n.ATAN&&l!==null&&(s="atan2"),e.shaderStage!=="fragment"&&(s===n.DFDX||s===n.DFDY)&&(_e(`TSL: '${s}' is not supported in the ${e.shaderStage} stage.`,this.stackTrace),s="/*"+s+"*/"),d.push(a.build(e,o)),l!==null&&d.push(l.build(e,o)),c!==null&&d.push(c.build(e,o))),e.format(`${e.getMethod(s,r)}( ${d.join(", ")} )`,r,t)}}serialize(e){super.serialize(e),e.method=this.method}deserialize(e){super.deserialize(e),this.method=e.method}};P.ALL="all";P.ANY="any";P.RADIANS="radians";P.DEGREES="degrees";P.EXP="exp";P.EXP2="exp2";P.LOG="log";P.LOG2="log2";P.SQRT="sqrt";P.INVERSE_SQRT="inversesqrt";P.FLOOR="floor";P.CEIL="ceil";P.NORMALIZE="normalize";P.FRACT="fract";P.SIN="sin";P.SINH="sinh";P.COS="cos";P.COSH="cosh";P.TAN="tan";P.TANH="tanh";P.ASIN="asin";P.ASINH="asinh";P.ACOS="acos";P.ACOSH="acosh";P.ATAN="atan";P.ATANH="atanh";P.ABS="abs";P.SIGN="sign";P.LENGTH="length";P.NEGATE="negate";P.ONE_MINUS="oneMinus";P.DFDX="dFdx";P.DFDY="dFdy";P.ROUND="round";P.RECIPROCAL="reciprocal";P.TRUNC="trunc";P.FWIDTH="fwidth";P.TRANSPOSE="transpose";P.DETERMINANT="determinant";P.INVERSE="inverse";P.EQUALS="equals";P.MIN="min";P.MAX="max";P.STEP="step";P.REFLECT="reflect";P.DISTANCE="distance";P.DIFFERENCE="difference";P.DOT="dot";P.CROSS="cross";P.POW="pow";P.TRANSFORM_DIRECTION="transformDirection";P.MIX="mix";P.CLAMP="clamp";P.REFRACT="refract";P.SMOOTHSTEP="smoothstep";P.FACEFORWARD="faceforward";var lR=it(Math.PI),uF=it(Math.PI*2),hF=it(Math.PI*2),dF=it(Math.PI*.5),cR=ge(P,P.ALL).setParameterLength(1),uR=ge(P,P.ANY).setParameterLength(1),hR=ge(P,P.RADIANS).setParameterLength(1),dR=ge(P,P.DEGREES).setParameterLength(1),fR=ge(P,P.EXP).setParameterLength(1),pR=ge(P,P.EXP2).setParameterLength(1),mR=ge(P,P.LOG).setParameterLength(1),gR=ge(P,P.LOG2).setParameterLength(1),Bx=ge(P,P.SQRT).setParameterLength(1),xR=ge(P,P.INVERSE_SQRT).setParameterLength(1),yR=ge(P,P.FLOOR).setParameterLength(1),_R=ge(P,P.CEIL).setParameterLength(1),pu=ge(P,P.NORMALIZE).setParameterLength(1),Fx=ge(P,P.FRACT).setParameterLength(1),Lx=ge(P,P.SIN).setParameterLength(1),bR=ge(P,P.SINH).setParameterLength(1),SR=ge(P,P.COS).setParameterLength(1),vR=ge(P,P.COSH).setParameterLength(1),AR=ge(P,P.TAN).setParameterLength(1),TR=ge(P,P.TANH).setParameterLength(1),ER=ge(P,P.ASIN).setParameterLength(1),MR=ge(P,P.ASINH).setParameterLength(1),RR=ge(P,P.ACOS).setParameterLength(1),wR=ge(P,P.ACOSH).setParameterLength(1),CR=ge(P,P.ATAN).setParameterLength(1,2),NR=ge(P,P.ATANH).setParameterLength(1),ep=ge(P,P.ABS).setParameterLength(1),Ux=ge(P,P.SIGN).setParameterLength(1),IR=ge(P,P.LENGTH).setParameterLength(1),BR=ge(P,P.NEGATE).setParameterLength(1),FR=ge(P,P.ONE_MINUS).setParameterLength(1),LR=ge(P,P.DFDX).setParameterLength(1),UR=ge(P,P.DFDY).setParameterLength(1),DR=ge(P,P.ROUND).setParameterLength(1),PR=ge(P,P.RECIPROCAL).setParameterLength(1),GR=ge(P,P.TRUNC).setParameterLength(1),VR=ge(P,P.FWIDTH).setParameterLength(1),OR=ge(P,P.TRANSPOSE).setParameterLength(1),kR=ge(P,P.DETERMINANT).setParameterLength(1),zR=ge(P,P.INVERSE).setParameterLength(1),WR=ge(P,P.MIN).setParameterLength(2,1/0),HR=ge(P,P.MAX).setParameterLength(2,1/0),Jd=ge(P,P.STEP).setParameterLength(2),YR=ge(P,P.REFLECT).setParameterLength(2),ZR=ge(P,P.DISTANCE).setParameterLength(2),XR=ge(P,P.DIFFERENCE).setParameterLength(2),mu=ge(P,P.DOT).setParameterLength(2),KR=ge(P,P.CROSS).setParameterLength(2),Dx=ge(P,P.POW).setParameterLength(2),$R=n=>zs(n,n),JR=n=>zs(n,n,n),qR=n=>zs(n,n,n,n),QR=ge(P,P.TRANSFORM_DIRECTION).setParameterLength(2),jR=(n,e)=>pu(zs(e,gt(wt(n),0)).xyz),ew=(n,e)=>pu(gt(wt(n),0).mul(e).xyz),tw=n=>zs(Ux(n),Dx(ep(n),1/3)),nw=n=>mu(n,n),ji=ge(P,P.MIX).setParameterLength(3),Px=(n,e=0,t=1)=>new P(P.CLAMP,we(n),we(e),we(t)),Gx=n=>Px(n),iw=ge(P,P.REFRACT).setParameterLength(3),Va=ge(P,P.SMOOTHSTEP).setParameterLength(3),sw=ge(P,P.FACEFORWARD).setParameterLength(3),rw=ke(([n])=>{let i=43758.5453,s=mu(n.xy,Vs(12.9898,78.233)),r=Ix(s,lR);return Fx(Lx(r).mul(i))}),ow=(n,e,t)=>ji(e,t,n),aw=(n,e,t)=>Va(e,t,n),lw=(n,e)=>Jd(e,n);W("all",cR);W("any",uR);W("radians",hR);W("degrees",dR);W("exp",fR);W("exp2",pR);W("log",mR);W("log2",gR);W("sqrt",Bx);W("inverseSqrt",xR);W("floor",yR);W("ceil",_R);W("normalize",pu);W("fract",Fx);W("sin",Lx);W("sinh",bR);W("cos",SR);W("cosh",vR);W("tan",AR);W("tanh",TR);W("asin",ER);W("asinh",MR);W("acos",RR);W("acosh",wR);W("atan",CR);W("atanh",NR);W("abs",ep);W("sign",Ux);W("length",IR);W("lengthSq",nw);W("negate",BR);W("oneMinus",FR);W("dFdx",LR);W("dFdy",UR);W("round",DR);W("reciprocal",PR);W("trunc",GR);W("fwidth",VR);W("min",WR);W("max",HR);W("step",lw);W("reflect",YR);W("distance",ZR);W("dot",mu);W("cross",KR);W("pow",Dx);W("pow2",$R);W("pow3",JR);W("pow4",qR);W("transformDirection",QR);W("transformNormalByViewMatrix",jR);W("transformNormalByInverseViewMatrix",ew);W("mix",ow);W("clamp",Px);W("refract",iw);W("smoothstep",aw);W("faceForward",sw);W("difference",XR);W("saturate",Gx);W("cbrt",tw);W("transpose",OR);W("determinant",kR);W("inverse",zR);W("rand",rw);var qd=class extends De{static get type(){return"ConditionalNode"}constructor(e,t,i=null){super(),this.condNode=e,this.ifNode=t,this.elseNode=i}generateNodeType(e){let{ifNode:t,elseNode:i}=e.getNodeProperties(this);if(t===void 0)return e.flowBuildStage(this,"setup"),this.getNodeType(e);let s=t.getNodeType(e);if(i!==null){let r=i.getNodeType(e);if(e.getTypeLength(r)>e.getTypeLength(s))return r}return s}setup(e){let t=this.condNode,i=this.ifNode.isolate(),s=this.elseNode?this.elseNode.isolate():null,r=e.context.nodeBlock;e.getDataFromNode(i).parentNodeBlock=r,s!==null&&(e.getDataFromNode(s).parentNodeBlock=r);let o=e.context.uniformFlow,a=e.getNodeProperties(this);a.condNode=t,a.ifNode=o?i:i.context({nodeBlock:i}),a.elseNode=s?o?s:s.context({nodeBlock:s}):null}generate(e,t){let i=this.getNodeType(e),s=e.getDataFromNode(this);if(s.nodeProperty!==void 0)return s.nodeProperty;let{condNode:r,ifNode:o,elseNode:a}=e.getNodeProperties(this),l=e.currentFunctionNode,c=t!=="void",u=c?ks(i).build(e):"";s.nodeProperty=u;let d=r.build(e,"bool");if(e.context.uniformFlow&&a!==null){let g=o.build(e,i),y=a.build(e,i),m=e.getTernary(d,g,y);return e.format(m,i,t)}e.addFlowCode(`
${e.tab}if ( ${d} ) {

`).addFlowTab();let f=o.build(e,i);if(f&&(c?f=u+" = "+f+";":(f="return "+f+";",l===null&&(pe("TSL: Return statement used in an inline 'Fn()'. Define a layout struct to allow return values.",this.stackTrace),f="// "+f))),e.removeFlowTab().addFlowCode(e.tab+"	"+f+`

`+e.tab+"}"),a!==null){e.addFlowCode(` else {

`).addFlowTab();let g=a.build(e,i);g&&(c?g=u+" = "+g+";":(g="return "+g+";",l===null&&(pe("TSL: Return statement used in an inline 'Fn()'. Define a layout struct to allow return values.",this.stackTrace),g="// "+g))),e.removeFlowTab().addFlowCode(e.tab+"	"+g+`

`+e.tab+`}

`)}else e.addFlowCode(`

`);return e.format(u,i,t)}},Vx=qt(qd).setParameterLength(2,3);W("select",Vx);var qc=class extends De{static get type(){return"ContextNode"}constructor(e=null,t={}){super(),this.isContextNode=!0,this.node=e,this.value=t}getScope(){return this.node.getScope()}generateNodeType(e){return this.node.getNodeType(e)}getFlowContextData(){let e=[];return this.traverse(t=>{t.isContextNode===!0&&e.push(t.value)}),Object.assign({},...e)}getMemberType(e,t){return this.node.getMemberType(e,t)}analyze(e){if(e.increaseUsage(this)===1){let i=e.addContext(this.value);this.node.build(e,this),e.setContext(i)}}setup(e){let t=e.addContext(this.value),i=this.node.build(e);return e.setContext(t),i}generate(e,t){let i=e.addContext(this.value),s=this.node.build(e,t);return e.setContext(i),s}},Kr=(n=null,e={})=>{let t=n;return(t===null||t.isNode!==!0)&&(e=t||e,t=null),new qc(t,e)},cw=n=>Kr(n,{uniformFlow:!0}),Ox=(n,e)=>Kr(n,{nodeName:e});function uw(n,e,t=null){return Kr(t,{getShadow:({light:i,shadowColorNode:s})=>e===i?s.mul(n):s})}function hw(n,e=null){return Kr(e,{getAO:(t,{material:i})=>i.transparent===!0?t:t!==null?t.mul(n):n})}function dw(n,e,t=null){let i=n.max(.001);return Kr(t,{getAO:(s,{material:r})=>r.transparent===!0?s:s!==null?s.mul(i):i,getGI:(s,{material:r})=>{if(r.transparent===!0)return s;let o=e.div(i);return s!==null?s.add(o):o}})}function fw(n,e){return pe('TSL: "label()" has been deprecated. Use "setName()" instead.'),Ox(n,e)}W("context",Kr);W("label",fw);W("uniformFlow",cw);W("setName",Ox);W("builtinShadowContext",(n,e,t)=>uw(e,t,n));W("builtinAOContext",(n,e)=>hw(e,n));W("builtinGIContext",(n,e,t)=>dw(e,t,n));var Qd=class extends De{static get type(){return"VarNode"}constructor(e,t=null,i=!1){super(),this.node=e,this.name=t,this.global=!0,this.isVarNode=!0,this.readOnly=i,this.parents=!0,this.intent=!1}setIntent(e){return this.intent=e,this}isIntent(e){return e.getDataFromNode(this).forceDeclaration===!0?!1:this.intent}getIntent(){return this.intent}getMemberType(e,t){return this.node.getMemberType(e,t)}getElementType(e){return this.node.getElementType(e)}generateNodeType(e){return this.node.getNodeType(e)}getArrayCount(e){return this.node.getArrayCount(e)}isAssign(e){return e.getDataFromNode(this).assign}build(...e){let t=e[0],i=this.getShared(t);if(this!==i)return i.build(...e);if(this._hasStack(t)===!1&&t.buildStage==="setup"&&(t.context.nodeLoop||t.context.nodeBlock)){let s=!1;if(this.node.isShaderCallNodeInternal&&this.node.shaderNode.getLayout()===null&&t.fnCall&&t.fnCall.shaderNode&&t.getDataFromNode(this.node.shaderNode).hasLoop){let a=t.getDataFromNode(this);a.forceDeclaration=!0,s=!0}let r=t.getBaseStack();s?r.addToStackBefore(this):r.addToStack(this)}return this.isIntent(t)&&this.isAssign(t)!==!0?this.node.build(...e):super.build(...e)}generate(e){let{node:t,name:i,readOnly:s}=this,{renderer:r}=e,o=r.backend.isWebGPUBackend===!0,a=!1,l=!1;s&&(a=e.isDeterministic(t),l=o?s:a);let c=this.getNodeType(e);if(c=="void")return this.isIntent(e)!==!0&&_e('TSL: ".toVar()" can not be used with void type.',this.stackTrace),t.build(e);let u=e.getVectorType(c),d=t.build(e,u),h=e.getVarFromNode(this,i,u,void 0,l),f=e.getPropertyName(h),g=f;if(l)if(o)g=a?`const ${f}`:`let ${f}`;else{let y=t.getArrayCount(e);g=`const ${e.getVar(h.type,f,y)}`}return e.addLineFlowCode(`${g} = ${d}`,this),f}_hasStack(e){return e.getDataFromNode(this).stack!==void 0}},tp=qt(Qd),pw=(n,e=null)=>tp(n,e).toStack(),mw=(n,e=null)=>tp(n,e,!0).toStack(),gw=n=>tp(n).setIntent(!0).toStack();W("toVar",pw);W("toConst",mw);W("toVarIntent",gw);var jd=class extends De{static get type(){return"SubBuild"}constructor(e,t,i=null){super(i),this.node=e,this.name=t,this.isSubBuildNode=!0}generateNodeType(e){if(this.nodeType!==null)return this.nodeType;e.addSubBuild(this.name);let t=this.node.getNodeType(e);return e.removeSubBuild(),t}build(e,...t){e.addSubBuild(this.name);let i=this.node.build(e,...t);return e.removeSubBuild(),i}},ux=(n,e,t=null)=>new jd(we(n),e,t),ef=class extends De{static get type(){return"VaryingNode"}constructor(e,t=null){super(),this.node=ux(e,"VERTEX"),this.name=t,this.isVaryingNode=!0,this.interpolationType=null,this.interpolationSampling=null,this.global=!0}setInterpolation(e,t=null){return this.interpolationType=e,this.interpolationSampling=t,this}getHash(e){return this.name||super.getHash(e)}generateNodeType(e){return this.node.getNodeType(e)}setupVarying(e){let t=e.getNodeProperties(this),i=t.varying;if(i===void 0){let s=this.name,r=this.getNodeType(e),o=this.interpolationType,a=this.interpolationSampling;t.varying=i=e.getVaryingFromNode(this,s,r,o,a),t.node=ux(this.node,"VERTEX")}return i.needsInterpolation||(i.needsInterpolation=e.shaderStage==="fragment"),i}setup(e){this.setupVarying(e),e.flowNodeFromShaderStage(wa.VERTEX,this.node)}analyze(e){this.setupVarying(e),e.flowNodeFromShaderStage(wa.VERTEX,this.node)}generate(e){let t=e.getSubBuildProperty("property",e.currentStack),i=e.getNodeProperties(this),s=this.setupVarying(e);if(i[t]===void 0){let r=this.getNodeType(e),o=e.getPropertyName(s,wa.VERTEX);if(e.shaderStage===wa.VERTEX){let a=i.node.build(e,r);e.addLineFlowCode(`${o} = ${a}`,this)}else e.flowNodeFromShaderStage(wa.VERTEX,i.node,r,o);i[t]=o}return e.getPropertyName(s)}},Ya=qt(ef).setParameterLength(1,2),xw=n=>Ya(n);W("toVarying",Ya);W("toVertexStage",xw);var yw=ke(([n])=>{let e=n.mul(.9478672986).add(.0521327014).pow(2.4),t=n.mul(.0773993808),i=n.lessThanEqual(.04045);return ji(e,t,i)}).setLayout({name:"sRGBTransferEOTF",type:"vec3",inputs:[{name:"color",type:"vec3"}]}),_w=ke(([n])=>{let e=n.pow(.41666).mul(1.055).sub(.055),t=n.mul(12.92),i=n.lessThanEqual(.0031308);return ji(e,t,i)}).setLayout({name:"sRGBTransferOETF",type:"vec3",inputs:[{name:"color",type:"vec3"}]}),np="WorkingColorSpace",bw="OutputColorSpace",Qc=class extends ln{static get type(){return"ColorSpaceNode"}constructor(e,t,i){super("vec4"),this.colorNode=e,this.source=t,this.target=i}resolveColorSpace(e,t){return t===np?Ve.workingColorSpace:t===bw?e.context.outputColorSpace||e.renderer.outputColorSpace:t}setup(e){let{colorNode:t}=this,i=this.resolveColorSpace(e,this.source),s=this.resolveColorSpace(e,this.target),r=t;return Ve.enabled===!1||i===s||!i||!s||(Ve.getTransfer(i)===nt&&(r=gt(yw(r.rgb),r.a)),Ve.getPrimaries(i)!==Ve.getPrimaries(s)&&(r=gt(Os(Ve._getMatrix(new Ge,i,s)).mul(r.rgb),r.a)),Ve.getTransfer(s)===nt&&(r=gt(_w(r.rgb),r.a))),r}},Sw=(n,e)=>new Qc(we(n),np,e),kx=(n,e)=>new Qc(we(n),e,np);W("workingToColorSpace",Sw);W("colorSpaceToWorking",kx);var jc=class extends Zr{static get type(){return"ReferenceElementNode"}constructor(e,t){super(e,t),this.referenceNode=e,this.isReferenceElementNode=!0}generateNodeType(){return this.referenceNode.uniformType}generate(e){let t=super.generate(e),i=this.referenceNode.getNodeType(e),s=this.getNodeType(e);return e.format(t,i,s)}},tf=class extends De{static get type(){return"ReferenceBaseNode"}constructor(e,t,i=null,s=null){super(),this.property=e,this.uniformType=t,this.object=i,this.count=s,this.properties=e.split("."),this.reference=i,this.node=null,this.group=null,this.updateType=ot.OBJECT}setGroup(e){return this.group=e,this}element(e){return new jc(this,we(e))}setNodeType(e){let t=Ht(null,e);this.group!==null&&t.setGroup(this.group),this.node=t}generateNodeType(e){return this.node===null&&(this.updateReference(e),this.updateValue()),this.node.getNodeType(e)}getValueFromReference(e=this.reference){let{properties:t}=this,i=e[t[0]];for(let s=1;s<t.length;s++)i=i[t[s]];return i}updateReference(e){return this.reference=this.object!==null?this.object:e.object,this.reference}setup(){return this.updateValue(),this.node}update(){this.updateValue()}updateValue(){this.node===null&&this.setNodeType(this.uniformType);let e=this.getValueFromReference();Array.isArray(e)?this.node.array=e:this.node.value=e}};var nf=class extends tf{static get type(){return"RendererReferenceNode"}constructor(e,t,i=null){super(e,t,i),this.renderer=i,this.setGroup(Wt)}updateReference(e){return this.reference=this.renderer!==null?this.renderer:e.renderer,this.reference}},vw=(n,e,t=null)=>new nf(n,e,t),sf=class extends ln{static get type(){return"ToneMappingNode"}constructor(e,t=Tw,i=null){super("vec3"),this._toneMapping=e,this.exposureNode=t,this.colorNode=i}customCacheKey(){return Tx(this._toneMapping)}setToneMapping(e){return this._toneMapping=e,this}getToneMapping(){return this._toneMapping}setup(e){let t=this.colorNode||e.context.color,i=this._toneMapping;if(i===pn)return t;let s=null,r=e.renderer.library.getToneMappingFunction(i);return r!==null?s=gt(r(t.rgb,this.exposureNode),t.a):(_e("ToneMappingNode: Unsupported Tone Mapping configuration.",i),s=t),s}},Aw=(n,e,t)=>new sf(n,we(e),we(t)),Tw=vw("toneMappingExposure","float");W("toneMapping",(n,e,t)=>Aw(e,t,n));var hx=new WeakMap;function dx(n,e){let t=hx.get(n);return t===void 0&&(t=new go(n,e),hx.set(n,t)),t}var _i=class extends Pa{static get type(){return"BufferAttributeNode"}constructor(e,t=null,i=0,s=0){super(e,t),this.isBufferNode=!0,this.bufferType=t,this.bufferStride=i,this.bufferOffset=s,this.usage=Lr,this.instanced=!1,this.attribute=null,this.global=!0,e&&e.isBufferAttribute===!0&&e.itemSize<=4&&(this.attribute=e,this.usage=e.usage,this.instanced=e.isInstancedBufferAttribute)}getHash(e){let t;if(this.bufferStride===0&&this.bufferOffset===0){let i=e.globalCache.getData(this.value);i===void 0&&(i={node:this},e.globalCache.setData(this.value,i)),t=i.node.id}else t=this.id;return String(t)}generateNodeType(e){return this.bufferType===null&&(this.bufferType=e.getTypeFromAttribute(this.attribute)),this.bufferType}setup(e){if(this.attribute!==null)return;let t=this.getNodeType(e),i=e.getTypeLength(t),s=this.value,r=this.bufferStride||i,o=this.bufferOffset,a;s.isInterleavedBuffer===!0?a=s:s.isBufferAttribute===!0?a=dx(s.array,r):a=dx(s,r);let l=new xo(a,i,o);a.setUsage(this.usage),this.attribute=l,this.attribute.isInstancedBufferAttribute=this.instanced}generate(e){let t=this.getNodeType(e),i=e.context.nodeName;i!==void 0&&delete e.context.nodeName;let s=e.getBufferAttributeFromNode(this,t,i),r=e.getPropertyName(s),o=null;if(e.shaderStage==="vertex"||e.shaderStage==="compute")this.name=r,o=r;else{let a;i&&(a=i+"Varying"),o=Ya(this,a).build(e,t)}return o}getInputType(){return"bufferAttribute"}setUsage(e){return this.usage=e,this.attribute&&this.attribute.isBufferAttribute===!0&&(this.attribute.usage=e),this}setInstanced(e){return this.instanced=e,this}};function Ew(n,e=null,t=0,i=0,s=Lr,r=!1){return e==="mat3"||e===null&&n.itemSize===9?Os(new _i(n,"vec3",9,0).setUsage(s).setInstanced(r),new _i(n,"vec3",9,3).setUsage(s).setInstanced(r),new _i(n,"vec3",9,6).setUsage(s).setInstanced(r)):e==="mat4"||e===null&&n.itemSize===16?Cx(new _i(n,"vec4",16,0).setUsage(s).setInstanced(r),new _i(n,"vec4",16,4).setUsage(s).setInstanced(r),new _i(n,"vec4",16,8).setUsage(s).setInstanced(r),new _i(n,"vec4",16,12).setUsage(s).setInstanced(r)):new _i(n,e,t,i).setUsage(s)}var Mw=(n,e=null,t=0,i=0)=>Ew(n,e,t,i);W("toAttribute",n=>Mw(n.value,n.bufferType));var Gt=class n extends De{static get type(){return"IndexNode"}constructor(e){super("uint"),this.scope=e,this.isIndexNode=!0}generate(e){let t=this.getNodeType(e),i=this.scope,s;if(i===n.VERTEX)s=e.getVertexIndex();else if(i===n.INSTANCE)s=e.getInstanceIndex();else if(i===n.DRAW)s=e.getDrawIndex();else if(i===n.INVOCATION_LOCAL)s=e.getInvocationLocalIndex();else if(i===n.INVOCATION_SUBGROUP)s=e.getInvocationSubgroupIndex();else if(i===n.SUBGROUP)s=e.getSubgroupIndex();else throw new Error("THREE.IndexNode: Unknown scope: "+i);let r;return e.shaderStage==="vertex"||e.shaderStage==="compute"?r=s:r=Ya(this).build(e,t),r}};Gt.VERTEX="vertex";Gt.INSTANCE="instance";Gt.SUBGROUP="subgroup";Gt.INVOCATION_LOCAL="invocationLocal";Gt.INVOCATION_SUBGROUP="invocationSubgroup";Gt.DRAW="draw";var fx=Ne(Gt,Gt.VERTEX),Rw=Ne(Gt,Gt.INSTANCE),fF=Ne(Gt,Gt.SUBGROUP),pF=Ne(Gt,Gt.INVOCATION_SUBGROUP),mF=Ne(Gt,Gt.INVOCATION_LOCAL),gF=Ne(Gt,Gt.DRAW),rf=class extends De{static get type(){return"ComputeNode"}constructor(e,t){super("void"),this.isComputeNode=!0,this.computeNode=e,this.workgroupSize=t,this.count=null,this.dispatchSize=null,this.version=1,this.name="",this.onInitFunction=null,this.countNode=null,this.updateBeforeType=ot.FRAME}setName(e){return this.name=e,this}label(e){return pe('TSL: "label()" has been deprecated. Use "setName()" instead.',new Gn),this.setName(e)}onInit(e){return this.onInitFunction=e,this}getUpdateBeforeType(e){return e.compute!==this?this.updateBeforeType:ot.NONE}updateBefore({renderer:e}){e.compute(this)}setup(e){this.count!==null&&this.countNode===null&&(this.countNode=Ht(this.count,"uint").onObjectUpdate(()=>this.count));let t=this.computeNode.build(e);if(t){let i=e.getNodeProperties(this);i.outputComputeNode=t.outputNode,t.outputNode=null}return t}generate(e,t){let{shaderStage:i}=e;if(i==="compute"){let s=this.computeNode.build(e,"void");if(s!==""&&e.addLineFlowCode(s,this),this.count!==null&&e.allowEarlyReturns===!0){let r=this.countNode.build(e,"uint"),o=Rw.build(e,"uint");e.flow.code=`${e.tab}if ( ${o} >= ${r} ) { return; }

${e.flow.code}`}}else{let r=e.getNodeProperties(this).outputComputeNode;if(r)return r.build(e,t)}}},zx=(n,e=[64])=>{(e.length===0||e.length>3)&&_e("TSL: compute() workgroupSize must have 1, 2, or 3 elements",new Gn);for(let t=0;t<e.length;t++){let i=e[t];(typeof i!="number"||i<=0||!Number.isInteger(i))&&_e(`TSL: compute() workgroupSize element at index [ ${t} ] must be a positive integer`,new Gn)}for(;e.length<3;)e.push(1);return new rf(we(n),e)},ww=(n,e,t)=>{let i=zx(n,t);return typeof e=="number"?i.count=e:i.dispatchSize=e,i};W("compute",ww);W("computeKernel",zx);var of=class extends De{static get type(){return"IsolateNode"}constructor(e,t=!0){super(),this.node=e,this.parent=t,this.isIsolateNode=!0}generateNodeType(e){let t=e.getCache(),i=e.getCacheFromNode(this,this.parent);e.setCache(i);let s=this.node.getNodeType(e);return e.setCache(t),s}build(e,...t){let i=e.getCache(),s=e.getCacheFromNode(this,this.parent);e.setCache(s);let r=this.node.build(e,...t);return e.setCache(i),r}setParent(e){return this.parent=e,this}getParent(){return this.parent}},Wx=n=>new of(we(n));function Cw(n,e=!0){return pe('TSL: "cache()" has been deprecated. Use "isolate()" instead.'),Wx(n).setParent(e)}W("cache",Cw);W("isolate",Wx);var af=class extends De{static get type(){return"BypassNode"}constructor(e,t){super(),this.isBypassNode=!0,this.outputNode=e,this.callNode=t}generateNodeType(e){return this.outputNode.getNodeType(e)}generate(e){let t=this.callNode.build(e,"void");return t!==""&&e.addLineFlowCode(t,this),this.outputNode.build(e)}},Nw=qt(af).setParameterLength(2);W("bypass",Nw);var Hx=ke(([n,e,t,i=it(0),s=it(1),r=du(!1)])=>{let o=n.sub(e).div(t.sub(e));return yM(r)&&(o=o.clamp()),o.mul(s.sub(i)).add(i)});function Iw(n,e,t,i=it(0),s=it(1)){return Hx(n,e,t,i,s,!0)}W("remap",Hx);W("remapClamp",Iw);var lf=class extends De{static get type(){return"ExpressionNode"}constructor(e="",t="void"){super(t),this.snippet=e}generate(e,t){let i=this.getNodeType(e),s=this.snippet;if(i==="void")e.addLineFlowCode(s,this);else return e.format(s,i,t)}},es=qt(lf).setParameterLength(1,2),Bw=n=>(n?Vx(n,es("discard")):es("discard")).toStack();W("discard",Bw);var Fw=ke(([n])=>gt(n.rgb.mul(n.a),n.a),{color:"vec4",return:"vec4"}),Lw=ke(([n])=>n.a.equal(0).select(gt(0),gt(n.rgb.div(n.a),n.a)),{color:"vec4",return:"vec4"}),cf=class extends ln{static get type(){return"RenderOutputNode"}constructor(e,t,i){super("vec4"),this.colorNode=e,this._toneMapping=t,this.outputColorSpace=i,this.isRenderOutputNode=!0}setToneMapping(e){return this._toneMapping=e,this}getToneMapping(){return this._toneMapping}setup({context:e}){let t=this.colorNode||e.color;t=gt(t.rgb,t.a.clamp(0,1)),t=Lw(t);let i=(this._toneMapping!==null?this._toneMapping:e.toneMapping)||pn,s=(this.outputColorSpace!==null?this.outputColorSpace:e.outputColorSpace)||Pn;return i!==pn&&(t=t.toneMapping(i)),s!==Pn&&s!==Ve.workingColorSpace&&(t=t.workingToColorSpace(s)),t=Fw(t),t}},Uw=(n,e=null,t=null)=>new cf(we(n),e,t);W("renderOutput",Uw);var uf=class extends ln{static get type(){return"DebugNode"}constructor(e,t=null){super(),this.node=e,this.callback=t}generateNodeType(e){return this.node.getNodeType(e)}setup(e){return this.node.build(e)}analyze(e){return this.node.build(e)}generate(e){let t=this.callback,i=this.node.build(e);if(t!==null)t(e,i);else{let s="--- TSL debug - "+e.shaderStage+" shader ---",r="-".repeat(s.length),o="";o+="// #"+s+`#
`,o+=e.flow.code.replace(/^\t/mg,"")+`
`,o+="/* ... */ "+i+` /* ... */
`,o+="// #"+r+`#
`,hs(o)}return i}},Dw=(n,e=null)=>new uf(we(n),e).toStack();W("debug",Dw);var hf=class extends wn{constructor(){super(),this._renderer=null,this.currentFrame=null,this.isRunning=!1,this.enabled=!0}get nodeFrame(){return this._renderer._nodes.nodeFrame}setRenderer(e){return this._renderer=e,this}getRenderer(){return this._renderer}begin(){this.isRunning=!0}finish(){this.isRunning=!1}inspect(){}computeAsync(){}beginCompute(){}finishCompute(){}beginRender(){}finishRender(){}copyTextureToTexture(){}copyFramebufferToTexture(){}dispose(){this.isRunning===!0&&this.finish(),this.dispatchEvent({type:"dispose"})}},df=class extends De{static get type(){return"InspectorNode"}constructor(e,t="",i=null){super(),this.node=e,this.name=t,this.callback=i,this.updateType=ot.FRAME,this.isInspectorNode=!0}getName(){return this.name||this.node.name}update(e){e.renderer.inspector.inspect(this)}generateNodeType(e){return this.node.getNodeType(e)}setup(e){let t=this.node;return e.context.inspector===!0&&this.callback!==null&&(t=this.callback(t)),e.renderer.backend.isWebGPUBackend!==!0&&e.renderer.inspector.constructor!==hf&&Zn('TSL: ".toInspector()" is only available with WebGPU.'),t}};function Pw(n,e="",t=null){return n=we(n),n.before(new df(n,e,t))}W("toInspector",Pw);var ff=class extends De{static get type(){return"AttributeNode"}constructor(e,t=null){super(t),this.global=!0,this._attributeName=e}getHash(e){return this.getAttributeName(e)}generateNodeType(e){let t=this.nodeType;if(t===null){let i=this.getAttributeName(e);if(e.hasGeometryAttribute(i)){let s=e.geometry.getAttribute(i);t=e.getTypeFromAttribute(s)}else t="float"}return t}setAttributeName(e){return this._attributeName=e,this}getAttributeName(){return this._attributeName}generate(e){let t=this.getAttributeName(e),i=this.getNodeType(e);if(e.hasGeometryAttribute(t)===!0){let r=e.geometry.getAttribute(t),o=e.getTypeFromAttribute(r),a=e.getAttribute(t,o);return e.shaderStage==="vertex"?e.format(a.name,o,i):Ya(this).build(e,i)}else return pe(`AttributeNode: Vertex attribute "${t}" not found on geometry.`),e.generateConst(i)}serialize(e){super.serialize(e),e.global=this.global,e._attributeName=this._attributeName}deserialize(e){super.deserialize(e),this.global=e.global,this._attributeName=e._attributeName}},ts=(n,e=null)=>new ff(n,e),gu=(n=0)=>ts("uv"+(n>0?n:""),"vec2"),pf=class extends De{static get type(){return"TextureSizeNode"}constructor(e,t=null){super("uvec2"),this.isTextureSizeNode=!0,this.textureNode=e,this.levelNode=t}generate(e){let t=this.textureNode.build(e,"property"),i=this.levelNode===null?"0":this.levelNode.build(e,"int"),s=e.generateTextureSize(this.textureNode.value,t,i);return e.format(s,this.getNodeType(e))}},px=qt(pf).setParameterLength(1,2),mf=class extends Ps{static get type(){return"MaxMipLevelNode"}constructor(e){super(0),this._textureNode=e,this.updateType=ot.FRAME}get textureNode(){return this._textureNode}get texture(){return this._textureNode.value}update(){let e=this.texture,t=e.images,i=t&&t.length>0?t[0]&&t[0].image||t[0]:e.image;if(i&&i.width!==void 0){let{width:s,height:r}=i;this.value=Math.log2(Math.max(s,r))}}},Gw=qt(mf).setParameterLength(1),gf=class extends Error{constructor(e,t=null){super(e),this.name="NodeError",this.stackTrace=t}},Yx=new $t,Oa=class extends Ps{static get type(){return"TextureNode"}constructor(e=Yx,t=null,i=null,s=null){super(e),this.isTextureNode=!0,this.uvNode=t,this.levelNode=i,this.biasNode=s,this.compareNode=null,this.depthNode=null,this.gradNode=null,this.gatherNode=null,this.offsetNode=null,this.sampler=!0,this.updateMatrix=!1,this.updateType=ot.NONE,this.referenceNode=null,this._value=e,this._matrixUniform=null,this._flipYUniform=null,this._samplerComparison=!1,this.setUpdateMatrix(t===null)}set value(e){this.referenceNode?this.referenceNode.value=e:this._value=e}get value(){return this.referenceNode?this.referenceNode.value:this._value}getUniformHash(){return this.value.uuid}generateNodeType(){return this.gatherNode!==null?"vec4":tM(this.value)}getInputType(){return"texture"}getDefaultUV(){return gu(this.value.channel)}updateReference(){return this.value}getTransformedUV(e){return this._matrixUniform===null&&(this._matrixUniform=Ht(this.value.matrix)),this._matrixUniform.mul(wt(e,1)).xy}setUpdateMatrix(e){return this.updateMatrix=e,this}setupUV(e,t){return e.isFlipY()&&(this._flipYUniform===null&&(this._flipYUniform=Ht(!1)),t=t.toVar(),this.sampler?t=this._flipYUniform.select(t.flipY(),t):t=this._flipYUniform.select(t.setY(hu(px(this,this.levelNode).y).sub(t.y).sub(1)),t)),t}setup(e){let t=e.getNodeProperties(this);t.referenceNode=this.referenceNode;let i=this.value;if(!i||i.isTexture!==!0)throw new gf("THREE.TSL: `texture( value )` function expects a valid instance of THREE.Texture().",this.stackTrace);let s=ke(()=>{let l=this.uvNode;return(l===null||e.context.forceUVContext===!0)&&e.context.getUV&&(l=e.context.getUV(this,e)),l||(l=this.getDefaultUV()),this.updateMatrix===!0&&(l=this.getTransformedUV(l)),l=this.setupUV(e,l),this.updateType=this._matrixUniform!==null||this._flipYUniform!==null?ot.OBJECT:ot.NONE,l})(),r=this.levelNode;r===null&&e.context.getTextureLevel&&(r=e.context.getTextureLevel(this));let o=null,a=null;if(this.compareNode!==null)if(e.renderer.hasCompatibility(Nh.TEXTURE_COMPARE))o=this.compareNode;else{let l=i.compareFunction;l===null||l===ma||l===ws||l===Fr||l===$i?a=this.compareNode:(o=this.compareNode,Zn('TSL: Only "LessCompare", "LessEqualCompare", "GreaterCompare" and "GreaterEqualCompare" are supported for depth texture comparison fallback.'))}t.uvNode=s,t.levelNode=r,t.biasNode=this.biasNode,t.compareNode=o,t.compareStepNode=a,t.gradNode=this.gradNode,t.gatherNode=this.gatherNode,t.depthNode=this.depthNode,t.offsetNode=this.offsetNode}generateUV(e,t){return t.build(e,this.sampler===!0?"vec2":"ivec2")}generateOffset(e,t){return t.build(e,"ivec2")}generateSnippet(e,t,i,s,r,o,a,l,c,u,d){let h=this.value,f;return r?f=e.generateTextureBias(h,t,i,r,o,u):l?f=e.generateTextureGrad(h,t,i,l,o,u):c?a?f=e.generateTextureGatherCompare(h,t,i,a,o,u,d):f=e.generateTextureGather(h,t,i,c,o,u,d):a?f=e.generateTextureCompare(h,t,i,a,o,u):this.sampler===!1?f=e.generateTextureLoad(h,t,i,s,o,u):s?f=e.generateTextureLevel(h,t,i,s,o,u):f=e.generateTexture(h,t,i,o,u),f}generate(e,t){let i=this.value,s=e.getNodeProperties(this),r=super.generate(e,"property");if(/^sampler/.test(t)){if(t==="samplerComparison"){this._samplerComparison=!0;let o=this.getSharedNode(e);o._samplerComparison=!0}return r+"_sampler"}else{if(e.isReference(t))return r;{let o=e.getDataFromNode(this),a=this.getNodeType(e),l=o.propertyName;if(l===void 0){let{uvNode:u,levelNode:d,biasNode:h,compareNode:f,compareStepNode:g,depthNode:y,gradNode:m,gatherNode:p,offsetNode:E}=s,w=this.generateUV(e,u),S=d?d.build(e,"float"):null,v=h?h.build(e,"float"):null,A=y?y.build(e,"int"):null,R=f?f.build(e,"float"):null,b=g?g.build(e,"float"):null,M=m?[m[0].build(e,"vec2"),m[1].build(e,"vec2")]:null,B=p?p.build(e,"int"):null,D=E?this.generateOffset(e,E):null,k=this._flipYUniform?this._flipYUniform.build(e,"bool"):null,H=A;H===null&&i.isArrayTexture&&this.isTexture3DNode!==!0&&(H="0");let F=e.getVarFromNode(this);l=e.getPropertyName(F);let O=this.generateSnippet(e,r,w,S,v,H,R,M,B,D,k),q;if(i.isDepthTexture===!0&&B===null?q="float":q=i.type===bn?"uvec4":i.type===Ts?"ivec4":"vec4",O=e.format(O,q,a),b!==null){let X=i.compareFunction;X===Fr||X===$i?O=Jd(es(O,a),es(b,"float")).build(e,a):O=Jd(es(b,"float"),es(O,a)).build(e,a)}e.addLineFlowCode(`${l} = ${O}`,this),o.snippet=O,o.propertyName=l}let c=l;return e.needsToWorkingColorSpace(i)&&(c=kx(es(c,a),i.colorSpace).setup(e).build(e,a)),e.format(c,a,t)}}}setSampler(e){return this.sampler=e,this}getSampler(){return this.sampler}sample(e){let t=this.clone();return t.uvNode=we(e),t.referenceNode=this.getBase(),we(t)}load(e){return this.sample(e).setSampler(!1)}blur(e){let t=this.clone();t.biasNode=we(e).mul(Gw(t)),t.referenceNode=this.getBase();let i=t.value;return t.generateMipmaps===!1&&(i&&i.generateMipmaps===!1||i.minFilter===Rt||i.magFilter===Rt)&&(pe("TSL: texture().blur() requires mipmaps and sampling. Use .generateMipmaps=true and .minFilter/.magFilter=THREE.LinearFilter in the Texture."),t.biasNode=null),we(t)}level(e){let t=this.clone();return t.levelNode=we(e),t.referenceNode=this.getBase(),we(t)}size(e){return px(this,e)}bias(e){let t=this.clone();return t.biasNode=we(e),t.referenceNode=this.getBase(),we(t)}getBase(){return this.referenceNode?this.referenceNode.getBase():this}compare(e){let t=this.clone();return t.compareNode=we(e),t.referenceNode=this.getBase(),we(t)}grad(e,t){let i=this.clone();return i.gradNode=[we(e),we(t)],i.referenceNode=this.getBase(),we(i)}gather(e=0){let t=this.clone();return t.gatherNode=we(e),t.referenceNode=this.getBase(),we(t)}isSampleCompare(){return this.compareNode!==null||this._samplerComparison===!0}depth(e){let t=this.clone();return t.depthNode=we(e),t.referenceNode=this.getBase(),we(t)}offset(e){let t=this.clone();return t.offsetNode=we(e),t.referenceNode=this.getBase(),we(t)}serialize(e){super.serialize(e),e.value=this.value.toJSON(e.meta).uuid,e.sampler=this.sampler,e.updateMatrix=this.updateMatrix,e.updateType=this.updateType}deserialize(e){super.deserialize(e),this.value=e.meta.textures[e.value],this.sampler=e.sampler,this.updateMatrix=e.updateMatrix,this.updateType=e.updateType}update(){let e=this.value,t=this._matrixUniform;t!==null&&(t.value=e.matrix),e.matrixAutoUpdate===!0&&e.updateMatrix();let i=this._flipYUniform;i!==null&&(i.value=e.image instanceof ImageBitmap&&e.flipY===!0||e.isRenderTargetTexture===!0||e.isFramebufferTexture===!0||e.isDepthTexture===!0)}clone(){let e=new this.constructor(this.value,this.uvNode,this.levelNode,this.biasNode);return e.sampler=this.sampler,e.depthNode=this.depthNode,e.compareNode=this.compareNode,e.gradNode=this.gradNode,e.gatherNode=this.gatherNode,e.offsetNode=this.offsetNode,e}},Vw=qt(Oa).setParameterLength(1,4).setName("texture"),Ow=(n=Yx,e=null,t=null,i=null)=>{let s;return n&&n.isTextureNode===!0?(s=we(n.clone()),s.referenceNode=n.getBase(),e!==null&&(s.uvNode=we(e)),t!==null&&(s.levelNode=we(t)),i!==null&&(s.biasNode=we(i))):s=Vw(n,e,t,i),s};var eu=class extends Ps{static get type(){return"BufferNode"}constructor(e,t,i=0){super(e,t),this.isBufferNode=!0,this.bufferType=t,this.bufferCount=i,this.updateRanges=[]}addUpdateRange(e,t){this.updateRanges.push({start:e,count:t})}clearUpdateRanges(){this.updateRanges.length=0}getElementType(e){return this.getNodeType(e)}getInputType(){return"buffer"}},kw=(n,e,t)=>new eu(n,e,t),xf=class extends Zr{static get type(){return"UniformArrayElementNode"}constructor(e,t){super(e,t),this.isArrayBufferElementNode=!0}generate(e){let t=super.generate(e),i=this.getNodeType(e),s=this.node.getPaddedType();return e.format(t,s,i)}},yf=class extends eu{static get type(){return"UniformArrayNode"}constructor(e,t=null){super(null),this.array=e,this.elementType=t===null?Xc(e[0]):t,this.paddedType=this.getPaddedType(),this.updateType=ot.RENDER,this.isArrayBufferNode=!0}generateNodeType(){return this.paddedType}getElementType(){return this.elementType}getPaddedType(){let e=this.elementType,t="vec4";return e==="mat2"?t="mat2":/mat/.test(e)===!0?t="mat4":e.charAt(0)==="i"?t="ivec4":e.charAt(0)==="u"&&(t="uvec4"),t}update(){this.updateBuffer()}onUpdate(e,t){return e=e.bind(this),super.onUpdate((i,s)=>{e(i,s),this.updateBuffer()},t)}updateBuffer(){let{array:e,value:t}=this,i=this.elementType;if(i==="float"||i==="int"||i==="uint")for(let s=0;s<e.length;s++){let r=s*4;t[r]=e[s]}else if(i==="color")for(let s=0;s<e.length;s++){let r=s*4,o=e[s];t[r]=o.r,t[r+1]=o.g,t[r+2]=o.b||0}else if(i==="mat2")for(let s=0;s<e.length;s++){let r=s*4,o=e[s];t[r]=o.elements[0],t[r+1]=o.elements[1],t[r+2]=o.elements[2],t[r+3]=o.elements[3]}else if(i==="mat3")for(let s=0;s<e.length;s++){let r=s*16,o=e[s];t[r]=o.elements[0],t[r+1]=o.elements[1],t[r+2]=o.elements[2],t[r+4]=o.elements[3],t[r+5]=o.elements[4],t[r+6]=o.elements[5],t[r+8]=o.elements[6],t[r+9]=o.elements[7],t[r+10]=o.elements[8],t[r+15]=1}else if(i==="mat4")for(let s=0;s<e.length;s++){let r=s*16,o=e[s];for(let a=0;a<o.elements.length;a++)t[r+a]=o.elements[a]}else for(let s=0;s<e.length;s++){let r=s*4,o=e[s];t[r]=o.x,t[r+1]=o.y,t[r+2]=o.z||0,t[r+3]=o.w||0}}setup(e){let t=this.array.length,i=this.elementType,s=Float32Array,r=this.paddedType,o=e.getTypeLength(r);return i.charAt(0)==="i"&&(s=Int32Array),i.charAt(0)==="u"&&(s=Uint32Array),this.value=new s(t*o),this.bufferCount=t,this.bufferType=r,this.updateBuffer(),super.setup(e)}element(e){return new xf(this,we(e))}},bi=(n,e)=>new yf(n,e),_f=class extends De{constructor(e){super("float"),this.name=e,this.isBuiltinNode=!0}generate(){return this.name}},Za=qt(_f).setParameterLength(1),Ca,Na,An=class n extends De{static get type(){return"ScreenNode"}constructor(e){super(),this.scope=e,this.isViewportNode=!0}generateNodeType(){return this.scope===n.VIEWPORT?"vec4":"vec2"}update({renderer:e}){let t=e.getRenderTarget();this.scope===n.VIEWPORT?t!==null?Na.copy(t.viewport):(e.getViewport(Na),Na.multiplyScalar(e.getPixelRatio())):t!==null?(Ca.width=t.width,Ca.height=t.height):e.getDrawingBufferSize(Ca)}setup(){let e=this.scope,t=null;e===n.SIZE?t=Ht(Ca||(Ca=new ze)).setGroup(Wt):e===n.VIEWPORT?t=Ht(Na||(Na=new dt)).setGroup(Wt):t=Vs(Zx.div(mx));let i=ot.NONE;return(this.scope===n.SIZE||this.scope===n.VIEWPORT)&&(i=ot.RENDER),this.updateType=i,t}generate(e){if(this.scope===n.COORDINATE){let t=e.getFragCoord();if(e.isFlipY()){let i=e.getNodeProperties(mx).outputNode.build(e);t=`${e.getType("vec2")}( ${t}.x, ${i}.y - ${t}.y )`}return t}return super.generate(e)}};An.COORDINATE="coordinate";An.VIEWPORT="viewport";An.SIZE="size";An.UV="uv";var zw=Ht(1).setGroup(Wt).onRenderUpdate(({renderer:n})=>n.getPixelRatio()),ip=Ne(An,An.UV),mx=Ne(An,An.SIZE),Zx=Ne(An,An.COORDINATE),Da=Ne(An,An.VIEWPORT),xF=Da.zw,yF=Zx.sub(Da.xy);var wd=null,zc=null,Cd=null,Wc=null,Nd=null,Hc=null,Id=null,Yc=null;var xu=Ht(0,"uint").setName("u_cameraIndex").setGroup(jf("cameraIndex")).toVarying("v_cameraIndex"),Ia=Ht("float").setName("cameraNear").setGroup(Wt).onRenderUpdate(({camera:n})=>n.near),Ba=Ht("float").setName("cameraFar").setGroup(Wt).onRenderUpdate(({camera:n})=>n.far),Hr=ke(({camera:n})=>{let e;if(n.isArrayCamera&&n.cameras.length>0){let t=[];for(let i of n.cameras)t.push(i.projectionMatrix);zc===null?zc=bi(t).setGroup(Wt).setName("cameraProjectionMatrices"):zc.array=t,e=zc.element(n.isMultiViewCamera?Za("gl_ViewID_OVR"):xu)}else wd===null&&(wd=Ht(n.projectionMatrix).setName("cameraProjectionMatrix").setGroup(Wt).onRenderUpdate(({camera:t})=>t.projectionMatrix)),e=wd;return e}).once()(),Ww=ke(({camera:n})=>{let e;if(n.isArrayCamera&&n.cameras.length>0){let t=[];for(let i of n.cameras)t.push(i.projectionMatrixInverse);Wc===null?Wc=bi(t).setGroup(Wt).setName("cameraProjectionMatricesInverse"):Wc.array=t,e=Wc.element(n.isMultiViewCamera?Za("gl_ViewID_OVR"):xu)}else Cd===null&&(Cd=Ht(n.projectionMatrixInverse).setName("cameraProjectionMatrixInverse").setGroup(Wt).onRenderUpdate(({camera:t})=>t.projectionMatrixInverse)),e=Cd;return e}).once()(),sp=ke(({camera:n})=>{let e;if(n.isArrayCamera&&n.cameras.length>0){let t=[];for(let i of n.cameras)t.push(i.matrixWorldInverse);Hc===null?Hc=bi(t).setGroup(Wt).setName("cameraViewMatrices"):Hc.array=t,e=Hc.element(n.isMultiViewCamera?Za("gl_ViewID_OVR"):xu)}else Nd===null&&(Nd=Ht(n.matrixWorldInverse).setName("cameraViewMatrix").setGroup(Wt).onRenderUpdate(({camera:t})=>t.matrixWorldInverse)),e=Nd;return e}).once()(),Xx=ke(({camera:n})=>{let e;if(n.isArrayCamera&&n.cameras.length>0){let t=[];for(let i of n.cameras)t.push(i.matrixWorld);Yc===null?Yc=bi(t).setGroup(Wt).setName("cameraWorldMatrices"):Yc.array=t,e=Yc.element(n.isMultiViewCamera?Za("gl_ViewID_OVR"):xu)}else Id===null&&(Id=Ht(n.matrixWorld).setName("cameraWorldMatrix").setGroup(Wt).onRenderUpdate(({camera:t})=>t.matrixWorld)),e=Id;return e}).once()();var gx=new Kn,Li=class n extends De{static get type(){return"Object3DNode"}constructor(e,t=null){super(),this.scope=e,this.object3d=t,this.updateType=ot.OBJECT,this.uniformNode=new Ps(null)}generateNodeType(){let e=this.scope;if(e===n.WORLD_MATRIX)return"mat4";if(e===n.POSITION||e===n.VIEW_POSITION||e===n.DIRECTION||e===n.SCALE)return"vec3";if(e===n.RADIUS)return"float"}update(e){let t=this.object3d,i=this.uniformNode,s=this.scope;if(s===n.WORLD_MATRIX)i.value=t.matrixWorld;else if(s===n.POSITION)i.value=i.value||new L,i.value.setFromMatrixPosition(t.matrixWorld);else if(s===n.SCALE)i.value=i.value||new L,i.value.setFromMatrixScale(t.matrixWorld);else if(s===n.DIRECTION)i.value=i.value||new L,t.getWorldDirection(i.value);else if(s===n.VIEW_POSITION){let r=e.camera;i.value=i.value||new L,i.value.setFromMatrixPosition(t.matrixWorld),i.value.applyMatrix4(r.matrixWorldInverse)}else if(s===n.RADIUS){let r=e.object.geometry;r.boundingSphere===null&&r.computeBoundingSphere(),gx.copy(r.boundingSphere).applyMatrix4(t.matrixWorld),i.value=gx.radius}}generate(e){let t=this.scope;return t===n.WORLD_MATRIX?this.uniformNode.nodeType="mat4":t===n.POSITION||t===n.VIEW_POSITION||t===n.DIRECTION||t===n.SCALE?this.uniformNode.nodeType="vec3":t===n.RADIUS&&(this.uniformNode.nodeType="float"),this.uniformNode.build(e)}serialize(e){super.serialize(e),e.scope=this.scope}deserialize(e){super.deserialize(e),this.scope=e.scope}};Li.WORLD_MATRIX="worldMatrix";Li.POSITION="position";Li.SCALE="scale";Li.VIEW_POSITION="viewPosition";Li.DIRECTION="direction";Li.RADIUS="radius";var Tn=class extends Li{static get type(){return"ModelNode"}constructor(e){super(e)}update(e){this.object3d=e.object,super.update(e)}},_F=Ne(Tn,Tn.DIRECTION),Kx=Ne(Tn,Tn.WORLD_MATRIX),bF=Ne(Tn,Tn.POSITION),SF=Ne(Tn,Tn.SCALE),vF=Ne(Tn,Tn.VIEW_POSITION),AF=Ne(Tn,Tn.RADIUS),Hw=Ht(new Ge).onObjectUpdate(({object:n},e)=>e.value.getNormalMatrix(n.matrixWorld));var bf=ke(n=>n.context.modelViewMatrix||Yw).once()().toVar("modelViewMatrix"),Yw=sp.mul(Kx);var Zw=ke(n=>n.shaderStage!=="fragment"?(Zn("TSL: `clipSpace` is only available in fragment stage."),gt()):n.context.clipSpace.toVarying("v_clipSpace")).once()(),ti=ts("position","vec3");var vn=ke(n=>{if(n.shaderStage==="fragment"&&n.material.vertexNode){let e=Ww.mul(Zw);return e.xyz.div(e.w).toVar("positionView")}return n.context.setupPositionView().toVarying("v_positionView")},"vec3").once(["POSITION","VERTEX"])(),$x=ke(n=>{let e;return n.camera.isOrthographicCamera?e=wt(0,0,1):e=vn.negate().toVarying("v_positionViewDirection").normalize(),e.toVar("positionViewDirection")},"vec3").once(["POSITION"])(),Sf=class extends De{static get type(){return"FrontFacingNode"}constructor(){super("bool"),this.isFrontFacingNode=!0}generate(e){if(e.shaderStage!=="fragment")return"true";let{material:t}=e;return t.side===Dt?"false":e.getFrontFacing()}},Xw=Ne(Sf),Jx=it(Xw).mul(2).sub(1),yu=ke(([n],{material:e})=>{let t=e.side;return t===Dt?n=n.mul(-1):t===sn&&(n=n.mul(Jx)),n});var Kw=ts("normal","vec3"),$w=ke(n=>n.geometry.hasAttribute("normal")===!1?(pe('TSL: Vertex attribute "normal" not found on geometry.'),wt(0,1,0)):Kw,"vec3").once()().toVar("normalLocal"),Jw=vn.dFdx().cross(vn.dFdy()).normalize().toVar("normalFlat"),qw=ke(n=>{let e;return n.isFlatShading()?e=Jw:e=qx($w).toVarying("v_normalViewGeometry").normalize(),e},"vec3").once()().toVar("normalViewGeometry");var In=ke(n=>{let e;return n.subBuildFn==="NORMAL"||n.subBuildFn==="VERTEX"?(e=qw,n.isFlatShading()!==!0&&(e=yu(e))):e=n.context.setupNormal().context({getUV:null,getTextureLevel:null}),e},"vec3").once(["NORMAL","VERTEX"])().toVar("normalView"),Qw=In.transformNormalByInverseViewMatrix(sp).toVar("normalWorld"),jw=ke(({subBuildFn:n,context:e})=>{let t;return n==="NORMAL"||n==="VERTEX"?t=In:t=e.setupClearcoatNormal().context({getUV:null,getTextureLevel:null}),t},"vec3").once(["NORMAL","VERTEX"])().toVar("clearcoatNormalView"),eC=ke(([n,e=Kx])=>Os(e).inverse().transpose().mul(n).normalize());W("transformNormal",eC);var qx=ke(([n],e)=>{let t=e.context.modelNormalViewMatrix;return t?n.transformNormalByViewMatrix(t):Hw.mul(n).transformNormalByViewMatrix(sp)}),TF=ke(()=>(pe('TSL: "transformedNormalView" is deprecated. Use "normalView" instead.'),In)).once(["NORMAL","VERTEX"])(),EF=ke(()=>(pe('TSL: "transformedNormalWorld" is deprecated. Use "normalWorld" instead.'),Qw)).once(["NORMAL","VERTEX"])(),MF=ke(()=>(pe('TSL: "transformedClearcoatNormalView" is deprecated. Use "clearcoatNormalView" instead.'),jw)).once(["NORMAL","VERTEX"])(),Bd=new ct,tC=Ht(0).onReference(({material:n})=>n).onObjectUpdate(({material:n})=>n.refractionRatio);var nC=Ht(new ct).onReference(function(n){return n.material}).onObjectUpdate(function({material:n,scene:e}){let i=(e.environment!==null||e.environmentNode&&e.environmentNode.isNode)&&n.envMap===null?e.environmentRotation:n.envMapRotation;return i?Bd.makeRotationFromEuler(i).transpose():Bd.identity(),Bd}),iC=$x.negate().reflect(In),sC=$x.negate().refract(In,tC),rC=iC.transformDirection(Xx).toVar("reflectVector"),oC=sC.transformDirection(Xx).toVar("refractVector"),aC=new xs,vf=class extends Oa{static get type(){return"CubeTextureNode"}constructor(e,t=null,i=null,s=null){super(e,t,i,s),this.isCubeTextureNode=!0}getInputType(){return this.value.isDepthTexture===!0?"cubeDepthTexture":"cubeTexture"}getDefaultUV(){let e=this.value;return e.mapping===hi?rC:e.mapping===Bi?oC:(_e('CubeTextureNode: Mapping "%s" not supported.',e.mapping),wt(0,0,0))}setUpdateMatrix(){}setupUV(e,t){let i=this.value;return i.isDepthTexture===!0?e.renderer.coordinateSystem===li?wt(t.x,t.y.negate(),t.z):t:(t=nC.mul(t),(e.renderer.coordinateSystem===li||!i.isRenderTargetTexture)&&(t=wt(t.x.negate(),t.yz)),t)}generateUV(e,t){return t.build(e,this.sampler===!0?"vec3":"ivec3")}},lC=qt(vf).setParameterLength(1,4).setName("cubeTexture"),cC=(n=aC,e=null,t=null,i=null)=>{let s;return n&&n.isCubeTextureNode===!0?(s=we(n.clone()),s.referenceNode=n,e!==null&&(s.uvNode=we(e)),t!==null&&(s.levelNode=we(t)),i!==null&&(s.biasNode=we(i))):s=lC(n,e,t,i),s};var tu=class extends De{static get type(){return"ReferenceNode"}constructor(e,t,i=null,s=null){super(),this.property=e,this.uniformType=t,this.object=i,this.count=s,this.properties=e.split("."),this.reference=i,this.node=null,this.group=null,this.name=null,this.updateType=ot.OBJECT}element(e){return new jc(this,we(e))}setGroup(e){return this.group=e,this}setName(e){return this.name=e,this}label(e){return pe('TSL: "label()" has been deprecated. Use "setName()" instead.'),this.setName(e)}setNodeType(e){let t=null;this.count!==null?t=kw(null,e,this.count):Array.isArray(this.getValueFromReference())?(t=bi(null,e),t.updateType=ot.OBJECT):e==="texture"?t=Ow(null):e==="cubeTexture"?t=cC(null):t=Ht(null,e),this.group!==null&&t.setGroup(this.group),this.name!==null&&t.setName(this.name),this.node=t}generateNodeType(e){return this.node===null&&(this.updateReference(e),this.updateValue()),this.node.getNodeType(e)}getValueFromReference(e=this.reference){let{properties:t}=this,i=e[t[0]];for(let s=1;s<t.length;s++)i=i[t[s]];return i}updateReference(e){return this.reference=this.object!==null?this.object:e.object,this.reference}setup(){return this.updateValue(),this.node}update(){this.updateValue()}updateValue(){this.node===null&&this.setNodeType(this.uniformType);let e=this.getValueFromReference();Array.isArray(e)?this.node.array=e:this.node.value=e}},xx=(n,e,t)=>new tu(n,e,t);var Af=class extends tu{static get type(){return"MaterialReferenceNode"}constructor(e,t,i=null){super(e,t,i),this.material=i,this.isMaterialReferenceNode=!0}updateReference(e){return this.reference=this.material!==null?this.material:e.material,this.reference}},uC=(n,e,t=null)=>new Af(n,e,t),Qx=gu(),hC=vn.dFdx(),dC=vn.dFdy(),jx=Qx.dFdx(),e0=Qx.dFdy(),t0=In,n0=dC.cross(t0),i0=t0.cross(hC),Tf=n0.mul(jx.x).add(i0.mul(e0.x)),Ef=n0.mul(jx.y).add(i0.mul(e0.y)),yx=Tf.dot(Tf).max(Ef.dot(Ef)),s0=yx.equal(0).select(0,yx.inverseSqrt()),fC=Tf.mul(s0).toVar("tangentViewFrame"),pC=Ef.mul(s0).toVar("bitangentViewFrame"),r0=ts("tangent","vec4"),mC=r0.xyz.toVar("tangentLocal"),o0=ke(n=>{let e;return n.subBuildFn==="VERTEX"||n.geometry.hasAttribute("tangent")?e=bf.mul(gt(mC,0)).xyz.toVarying("v_tangentView").normalize():e=fC,n.isFlatShading()!==!0&&(e=yu(e)),e},"vec3").once(["NORMAL","VERTEX"])().toVar("tangentView");var gC=ke(([n,e],t)=>{let i=n.mul(r0.w).xyz;return t.subBuildFn==="NORMAL"&&t.isFlatShading()!==!0&&(i=i.toVarying(e)),i}).once(["NORMAL"]);var xC=ke(n=>{let e;return n.subBuildFn==="VERTEX"||n.geometry.hasAttribute("tangent")?e=gC(In.cross(o0),"v_bitangentView").normalize():e=pC,n.isFlatShading()!==!0&&(e=yu(e)),e},"vec3").once(["NORMAL","VERTEX"])().toVar("bitangentView");var yC=Os(o0,xC,In).toVar("TBNViewMatrix");var _x=n=>wt(n,Bx(Gx(it(1).sub(mu(n,n)))));var Mf=class extends ln{static get type(){return"NormalMapNode"}constructor(e,t=null){super("vec3"),this.node=e,this.scaleNode=t,this.normalMapType=Ki,this.unpackNormalMode=pa}setup(e){let{normalMapType:t,scaleNode:i,unpackNormalMode:s}=this,r=this.node.mul(2).sub(1);if(t===Ki?s===ac?r=_x(r.xy):s===Rh?r=_x(r.yw):s!==pa&&_e(`THREE.NodeMaterial: Unexpected unpack normal mode: ${s}`):s!==pa&&_e(`THREE.NodeMaterial: Normal map type '${t}' is not compatible with unpack normal mode '${s}'`),i!==null){let a=i;e.isFlatShading()===!0&&(a=yu(a)),r=wt(r.xy.mul(a),r.z)}let o=null;return t===oc?o=qx(r):t===Ki?o=yC.mul(r).normalize():(_e(`NodeMaterial: Unsupported normal map type: ${t}`),o=In),o}},bx=qt(Mf).setParameterLength(1,2),_C=ke(({textureNode:n,bumpScale:e})=>{let t=s=>n.isolate().context({getUV:r=>s(r.uvNode||gu()),forceUVContext:!0}),i=it(t(s=>s));return Vs(it(t(s=>s.add(s.dFdx()))).sub(i),it(t(s=>s.add(s.dFdy()))).sub(i)).mul(e)}),bC=ke(n=>{let{surf_pos:e,surf_norm:t,dHdxy:i}=n,s=e.dFdx().normalize(),r=e.dFdy().normalize(),o=t,a=r.cross(o),l=o.cross(s),c=s.dot(a).mul(Jx),u=c.sign().mul(i.x.mul(a).add(i.y.mul(l)));return c.abs().mul(t).sub(u).normalize()}),Rf=class extends ln{static get type(){return"BumpMapNode"}constructor(e,t=null){super("vec3"),this.textureNode=e,this.scaleNode=t}setup(e){if(e.material.wireframe===!0)return In;let t=this.scaleNode!==null?this.scaleNode:1,i=_C({textureNode:this.textureNode,bumpScale:t});return bC({surf_pos:vn,surf_norm:In,dHdxy:i})}},SC=qt(Rf).setParameterLength(1,2),Sx=new Map,ee=class n extends De{static get type(){return"MaterialNode"}constructor(e){super(),this.scope=e}getCache(e,t){let i=Sx.get(e);return i===void 0&&(i=uC(e,t),Sx.set(e,i)),i}getFloat(e){return this.getCache(e,"float")}getColor(e){return this.getCache(e,"color")}getTexture(e){return this.getCache(e==="map"?"map":e+"Map","texture")}setup(e){let t=e.context.material,i=this.scope,s=null;if(i===n.COLOR){let r=t.color!==void 0?this.getColor(i):wt();t.map&&t.map.isTexture===!0?s=r.mul(this.getTexture("map")):s=r}else if(i===n.OPACITY){let r=this.getFloat(i);t.alphaMap&&t.alphaMap.isTexture===!0?s=r.mul(this.getTexture("alpha")):s=r}else if(i===n.SPECULAR_STRENGTH)t.specularMap&&t.specularMap.isTexture===!0?s=this.getTexture("specular").r:s=it(1);else if(i===n.SPECULAR_INTENSITY){let r=this.getFloat(i);t.specularIntensityMap&&t.specularIntensityMap.isTexture===!0?s=r.mul(this.getTexture(i).a):s=r}else if(i===n.SPECULAR_COLOR){let r=this.getColor(i);t.specularColorMap&&t.specularColorMap.isTexture===!0?s=r.mul(this.getTexture(i).rgb):s=r}else if(i===n.ROUGHNESS){let r=this.getFloat(i);t.roughnessMap&&t.roughnessMap.isTexture===!0?s=r.mul(this.getTexture(i).g):s=r}else if(i===n.METALNESS){let r=this.getFloat(i);t.metalnessMap&&t.metalnessMap.isTexture===!0?s=r.mul(this.getTexture(i).b):s=r}else if(i===n.EMISSIVE){let r=this.getFloat("emissiveIntensity"),o=this.getColor(i).mul(r);t.emissiveMap&&t.emissiveMap.isTexture===!0?s=o.mul(this.getTexture(i)):s=o}else if(i===n.NORMAL)t.normalMap?(s=bx(this.getTexture("normal"),this.getCache("normalScale","vec2")),s.normalMapType=t.normalMapType,(t.normalMap.format==Qn||t.normalMap.format==Rs||t.normalMap.format==Ms)&&(s.unpackNormalMode=ac)):t.bumpMap?s=SC(this.getTexture("bump").r,this.getFloat("bumpScale")):s=In;else if(i===n.CLEARCOAT){let r=this.getFloat(i);t.clearcoatMap&&t.clearcoatMap.isTexture===!0?s=r.mul(this.getTexture(i).r):s=r}else if(i===n.CLEARCOAT_ROUGHNESS){let r=this.getFloat(i);t.clearcoatRoughnessMap&&t.clearcoatRoughnessMap.isTexture===!0?s=r.mul(this.getTexture(i).r):s=r}else if(i===n.CLEARCOAT_NORMAL)t.clearcoatNormalMap?s=bx(this.getTexture(i),this.getCache(i+"Scale","vec2")):s=In;else if(i===n.SHEEN){let r=this.getColor("sheenColor").mul(this.getFloat("sheen"));t.sheenColorMap&&t.sheenColorMap.isTexture===!0?s=r.mul(this.getTexture("sheenColor").rgb):s=r}else if(i===n.SHEEN_ROUGHNESS){let r=this.getFloat(i);t.sheenRoughnessMap&&t.sheenRoughnessMap.isTexture===!0?s=r.mul(this.getTexture(i).a):s=r,s=s.clamp(1e-4,1)}else if(i===n.ANISOTROPY)if(t.anisotropyMap&&t.anisotropyMap.isTexture===!0){let r=this.getTexture(i);s=wx(Fa.x,Fa.y,Fa.y.negate(),Fa.x).mul(r.rg.mul(2).sub(Vs(1)).normalize().mul(r.b))}else s=Fa;else if(i===n.IRIDESCENCE_THICKNESS){let r=xx("1","float",t.iridescenceThicknessRange);if(t.iridescenceThicknessMap){let o=xx("0","float",t.iridescenceThicknessRange);s=r.sub(o).mul(this.getTexture(i).g).add(o)}else s=r}else if(i===n.TRANSMISSION){let r=this.getFloat(i);t.transmissionMap?s=r.mul(this.getTexture(i).r):s=r}else if(i===n.THICKNESS){let r=this.getFloat(i);t.thicknessMap?s=r.mul(this.getTexture(i).g):s=r}else if(i===n.IOR)s=this.getFloat(i);else if(i===n.LIGHT_MAP)t.lightMap?s=this.getTexture(i).rgb.mul(this.getFloat("lightMapIntensity")):s=wt(0);else if(i===n.AO)t.aoMap?s=this.getTexture(i).r.sub(1).mul(this.getFloat("aoMapIntensity")).add(1):s=it(1);else if(i===n.LINE_DASH_OFFSET)s=t.dashOffset?this.getFloat(i):it(0);else{let r=this.getNodeType(e);s=this.getCache(i,r)}return s}};ee.ALPHA_TEST="alphaTest";ee.COLOR="color";ee.OPACITY="opacity";ee.SHININESS="shininess";ee.SPECULAR="specular";ee.SPECULAR_STRENGTH="specularStrength";ee.SPECULAR_INTENSITY="specularIntensity";ee.SPECULAR_COLOR="specularColor";ee.REFLECTIVITY="reflectivity";ee.ROUGHNESS="roughness";ee.METALNESS="metalness";ee.NORMAL="normal";ee.CLEARCOAT="clearcoat";ee.CLEARCOAT_ROUGHNESS="clearcoatRoughness";ee.CLEARCOAT_NORMAL="clearcoatNormal";ee.EMISSIVE="emissive";ee.ROTATION="rotation";ee.SHEEN="sheen";ee.SHEEN_ROUGHNESS="sheenRoughness";ee.ANISOTROPY="anisotropy";ee.IRIDESCENCE="iridescence";ee.IRIDESCENCE_IOR="iridescenceIOR";ee.IRIDESCENCE_THICKNESS="iridescenceThickness";ee.IOR="ior";ee.TRANSMISSION="transmission";ee.THICKNESS="thickness";ee.ATTENUATION_DISTANCE="attenuationDistance";ee.ATTENUATION_COLOR="attenuationColor";ee.LINE_SCALE="scale";ee.LINE_DASH_SIZE="dashSize";ee.LINE_GAP_SIZE="gapSize";ee.LINE_WIDTH="linewidth";ee.LINE_DASH_OFFSET="dashOffset";ee.POINT_SIZE="size";ee.DISPERSION="dispersion";ee.RETROREFLECTIVITY="retroreflectivity";ee.LIGHT_MAP="light";ee.AO="ao";var RF=Ne(ee,ee.ALPHA_TEST),wF=Ne(ee,ee.COLOR),CF=Ne(ee,ee.SHININESS),NF=Ne(ee,ee.EMISSIVE),IF=Ne(ee,ee.OPACITY),BF=Ne(ee,ee.SPECULAR),FF=Ne(ee,ee.SPECULAR_INTENSITY),LF=Ne(ee,ee.SPECULAR_COLOR),UF=Ne(ee,ee.SPECULAR_STRENGTH),DF=Ne(ee,ee.REFLECTIVITY),PF=Ne(ee,ee.ROUGHNESS),GF=Ne(ee,ee.METALNESS),VF=Ne(ee,ee.NORMAL),OF=Ne(ee,ee.CLEARCOAT),kF=Ne(ee,ee.CLEARCOAT_ROUGHNESS),zF=Ne(ee,ee.CLEARCOAT_NORMAL),WF=Ne(ee,ee.ROTATION),HF=Ne(ee,ee.SHEEN),YF=Ne(ee,ee.SHEEN_ROUGHNESS),ZF=Ne(ee,ee.ANISOTROPY),XF=Ne(ee,ee.IRIDESCENCE),KF=Ne(ee,ee.IRIDESCENCE_IOR),$F=Ne(ee,ee.IRIDESCENCE_THICKNESS),JF=Ne(ee,ee.TRANSMISSION),qF=Ne(ee,ee.THICKNESS),QF=Ne(ee,ee.IOR),jF=Ne(ee,ee.ATTENUATION_DISTANCE),eL=Ne(ee,ee.ATTENUATION_COLOR),vC=Ne(ee,ee.LINE_SCALE),AC=Ne(ee,ee.LINE_DASH_SIZE),TC=Ne(ee,ee.LINE_GAP_SIZE),wf=Ne(ee,ee.LINE_WIDTH),EC=Ne(ee,ee.LINE_DASH_OFFSET),tL=Ne(ee,ee.POINT_SIZE),nL=Ne(ee,ee.DISPERSION),iL=Ne(ee,ee.RETROREFLECTIVITY),sL=Ne(ee,ee.LIGHT_MAP),rL=Ne(ee,ee.AO),Fa=Ht(new ze).onReference(function(n){return n.material}).onRenderUpdate(function({material:n}){this.value.set(n.anisotropy*Math.cos(n.anisotropyRotation),n.anisotropy*Math.sin(n.anisotropyRotation))});var ni=class n extends De{static get type(){return"EventNode"}constructor(e,t){super("void"),this.eventType=e,this.callback=t,e===n.OBJECT?this.updateType=ot.OBJECT:e===n.MATERIAL?this.updateType=ot.RENDER:e===n.FRAME?this.updateType=ot.FRAME:e===n.AFTER_OBJECT?this.updateAfterType=ot.OBJECT:e===n.BEFORE_OBJECT?this.updateBeforeType=ot.OBJECT:e===n.BEFORE_MATERIAL?this.updateBeforeType=ot.RENDER:e===n.BEFORE_FRAME&&(this.updateBeforeType=ot.FRAME)}setup(e){let{eventType:t,callback:i}=this;if(t===n.BEFORE_RENDER_PIPELINE){let s=e.context.onBeforePipelineCallbacks;s&&s.push(i)}else if(t===n.AFTER_RENDER_PIPELINE){let s=e.context.onAfterPipelineCallbacks;s&&s.push(i)}return super.setup(e)}update(e){this.callback(e)}updateBefore(e){this.callback(e)}updateAfter(e){this.callback(e)}};ni.OBJECT="object";ni.MATERIAL="material";ni.FRAME="frame";ni.AFTER_OBJECT="afterObject";ni.BEFORE_OBJECT="beforeObject";ni.BEFORE_MATERIAL="beforeMaterial";ni.BEFORE_FRAME="beforeFrame";ni.BEFORE_RENDER_PIPELINE="beforeRenderPipeline";ni.AFTER_RENDER_PIPELINE="afterRenderPipeline";var Cf=class extends De{static get type(){return"LoopNode"}constructor(e=[]){super("void"),this.params=e}getVarName(e){return String.fromCharCode(105+e)}getProperties(e){let t=e.getNodeProperties(this);if(t.stackNode!==void 0)return t;let i={},s=this._getInternalParams();for(let l=0,c=s.length-1;l<c;l++){let u=s[l],d=u.isNode!==!0&&u.name||this.getVarName(l),h=u.isNode!==!0&&u.type||"int";i[d]=es(d,h)}let r=e.addStack(),o=s[s.length-1](i);t.returnsNode=o.context({nodeLoop:o}),t.stackNode=r;let a=s[0];if(a.isNode!==!0&&typeof a.update=="function"){let l=ke(a.update)(i);t.updateNode=l.context({nodeLoop:l})}return e.removeStack(),t}_getInternalParams(){let e=this.params;return typeof e[0]=="function"?[du(!0),e[0]]:e}setup(e){if(this.getProperties(e),e.fnCall){let t=e.getDataFromNode(e.fnCall.shaderNode);t.hasLoop=!0}}generate(e){let t=this.getProperties(e),i=this._getInternalParams(),s=t.stackNode;for(let o=0,a=i.length-1;o<a;o++){let l=i[o],c=!1,u=null,d=null,h=null,f=null,g=null,y=null;l.isNode?l.getNodeType(e)==="bool"?(c=!0,f="bool",d=l.build(e,f)):(f="int",h=this.getVarName(o),u="0",d=l.build(e,f),g="<"):(f=l.type||"int",h=l.name||this.getVarName(o),u=l.start,d=l.end,g=l.condition,y=l.update,typeof u=="number"?u=e.generateConst(f,u):u&&u.isNode&&(u=u.build(e,f)),typeof d=="number"?d=e.generateConst(f,d):d&&d.isNode&&(d=d.build(e,f)),u!==void 0&&d===void 0?(u=u+" - 1",d="0",g=">="):d!==void 0&&u===void 0&&(u="0",g="<"),g===void 0&&(Number(u)>Number(d)?g=">=":g="<"));let m;if(c)m=`while ( ${d} )`;else{let p={start:u,end:d},E=p.start,w=p.end,S,v=()=>g.includes("<")?"+=":"-=";if(y!=null)switch(typeof y){case"function":S=e.flowStagesNode(t.updateNode,"void").code.replace(/\t|;/g,"");break;case"number":S=h+" "+v()+" "+e.generateConst(f,y);break;case"string":S=h+" "+y;break;default:y.isNode?S=h+" "+v()+" "+y.build(e):(_e("TSL: 'Loop( { update: ... } )' is not a function, string or number.",this.stackTrace),S="break /* invalid update */")}else f==="int"||f==="uint"?y=g.includes("<")?"++":"--":y=v()+" 1.",S=h+" "+y;let A=e.getVar(f,h)+" = "+E,R=h+" "+g+" "+w;m=`for ( ${A}; ${R}; ${S} )`}e.addFlowCode((o===0?`
`:"")+e.tab+m+` {

`).addFlowTab()}let r=s.build(e,"void");t.returnsNode.build(e,"void"),e.removeFlowTab().addFlowCode(`
`+e.tab+r);for(let o=0,a=i.length-1;o<a;o++)e.addFlowCode((o===0?"":e.tab)+`}

`).removeFlowTab();e.addFlowTab()}},La=(...n)=>new Cf(Yr(n,"int")).toStack();var Ls=new ze,Nf=class extends Oa{static get type(){return"ViewportTextureNode"}constructor(e=ip,t=null,i=null){let s=null;i===null?(s=new _o,s.minFilter=di,i=s):s=i,super(i,e,t),this.generateMipmaps=!1,this.defaultFramebuffer=s,this.isOutputTextureNode=!0,this.updateBeforeType=ot.RENDER,this._cacheTextures=new WeakMap}getTextureForReference(e=null){let t,i;if(this.referenceNode?(t=this.referenceNode.defaultFramebuffer,i=this.referenceNode._cacheTextures):(t=this.defaultFramebuffer,i=this._cacheTextures),e===null)return t;if(i.has(e)===!1){let s=t.clone();i.set(e,s)}return i.get(e)}updateReference(e){let t=e.renderer,i=t.getRenderTarget(),s=t.getCanvasTarget(),r=i||s;return this.value=this.getTextureForReference(r),this.value}updateBefore(e){let t=e.renderer,i=t.getRenderTarget(),s=t.getCanvasTarget(),r=i||s;r===null?t.getDrawingBufferSize(Ls):r.getDrawingBufferSize?r.getDrawingBufferSize(Ls):Ls.set(r.width,r.height);let o=this.getTextureForReference(r);(o.image.width!==Ls.width||o.image.height!==Ls.height)&&(o.image.width=Ls.width,o.image.height=Ls.height,o.needsUpdate=!0);let a=o.generateMipmaps;o.generateMipmaps=this.generateMipmaps,t.copyFramebufferToTexture(o),o.generateMipmaps=a}clone(){let e=new this.constructor(this.uvNode,this.levelNode,this.value);return e.generateMipmaps=this.generateMipmaps,e}};var Fd=null,If=class extends Nf{static get type(){return"ViewportDepthTextureNode"}constructor(e=ip,t=null,i=null){i===null&&(Fd===null&&(Fd=new $n),i=Fd),super(e,t,i)}},MC=qt(If).setParameterLength(0,3),ii=class n extends De{static get type(){return"ViewportDepthNode"}constructor(e,t=null){super("float"),this.scope=e,this.valueNode=t,this.isViewportDepthNode=!0}generate(e){let{scope:t}=this;return t===n.DEPTH_BASE?e.getFragDepth():super.generate(e)}setup({camera:e}){let{scope:t}=this,i=this.valueNode,s=null;if(t===n.DEPTH_BASE)i!==null&&(s=a0().assign(i));else if(t===n.DEPTH)e.isPerspectiveCamera?s=RC(vn.z,Ia,Ba):s=Ld(vn.z,Ia,Ba);else if(t===n.LINEAR_DEPTH)if(i!==null)if(e.isPerspectiveCamera){let r=wC(i,Ia,Ba);s=Ld(r,Ia,Ba)}else s=i;else s=Ld(vn.z,Ia,Ba);return s}};ii.DEPTH_BASE="depthBase";ii.DEPTH="depth";ii.LINEAR_DEPTH="linearDepth";var Ld=(n,e,t)=>n.add(e).div(e.sub(t));var RC=(n,e,t)=>e.add(n).mul(t).div(t.sub(e).mul(n));var wC=ke(([n,e,t],i)=>i.renderer.reversedDepthBuffer===!0?e.mul(t).div(e.sub(t).mul(n).sub(e)):e.mul(t).div(t.sub(e).mul(n).sub(t)));var a0=qt(ii,ii.DEPTH_BASE),CC=Ne(ii,ii.DEPTH),NC=qt(ii,ii.LINEAR_DEPTH).setParameterLength(0,1),oL=NC(MC());CC.assign=n=>a0(n);var ka=class n extends De{static get type(){return"ClippingNode"}constructor(e=n.DEFAULT){super(),this.scope=e}setup(e){super.setup(e);let t=e.clippingContext,{intersectionPlanes:i,unionPlanes:s}=t;return this.hardwareClipping=e.hardwareClipping,this.scope===n.ALPHA_TO_COVERAGE?this.setupAlphaToCoverage(i,s):this.scope===n.HARDWARE?this.setupHardwareClipping(s,e):this.setupDefault(i,s)}setupAlphaToCoverage(e,t){return ke(()=>{let i=it().toVar("distanceToPlane"),s=it().toVar("distanceToGradient"),r=it(1).toVar("clipOpacity"),o=t.length;if(this.hardwareClipping===!1&&o>0){let l=bi(t).setGroup(Wt);La(o,({i:c})=>{let u=l.element(c);i.assign(vn.dot(u.xyz).negate().add(u.w)),s.assign(i.fwidth().div(2)),r.mulAssign(Va(s.negate(),s,i))})}let a=e.length;if(a>0){let l=bi(e).setGroup(Wt),c=it(1).toVar("intersectionClipOpacity");La(a,({i:u})=>{let d=l.element(u);i.assign(vn.dot(d.xyz).negate().add(d.w)),s.assign(i.fwidth().div(2)),c.mulAssign(Va(s.negate(),s,i).oneMinus())}),r.mulAssign(c.oneMinus())}lx.a.mulAssign(r),lx.a.equal(0).discard()})()}setupDefault(e,t){return ke(()=>{let i=t.length;if(this.hardwareClipping===!1&&i>0){let r=bi(t).setGroup(Wt);La(i,({i:o})=>{let a=r.element(o);vn.dot(a.xyz).greaterThan(a.w).discard()})}let s=e.length;if(s>0){let r=bi(e).setGroup(Wt),o=du(!0).toVar("clipped");La(s,({i:a})=>{let l=r.element(a);o.assign(vn.dot(l.xyz).greaterThan(l.w).and(o))}),o.discard()}})()}setupHardwareClipping(e,t){let i=e.length;return t.enableHardwareClipping(i),ke(()=>{let s=bi(e).setGroup(Wt),r=Za(t.getClipDistance());La(i,({i:o})=>{let a=s.element(o),l=vn.dot(a.xyz).sub(a.w).negate();r.element(o).assign(l)})})()}};ka.ALPHA_TO_COVERAGE="alphaToCoverage";ka.DEFAULT="default";ka.HARDWARE="hardware";var Zc=fu("vec3","worldStart"),Bf=fu("vec3","worldEnd"),l0=fu("float","lineDistance"),Us=fu("vec4","worldPos"),vx=ke(({start:n,end:e})=>{let t=Hr.element(2).element(2),i=Hr.element(3).element(2);return t.greaterThan(0).select(i.negate().div(t.add(1)),i.mul(-.5).div(t)).sub(n.z).div(e.z.sub(n.z))},{start:"vec4",end:"vec4",return:"float"}),IC=ke(({p1:n,p2:e,p3:t,p4:i})=>{let s=n.sub(t),r=i.sub(t),o=e.sub(n),a=s.dot(r),l=r.dot(o),c=s.dot(o),u=r.dot(r),h=o.dot(o).mul(u).sub(l.mul(l)),g=a.mul(l).sub(c.mul(u)).div(h).clamp(),y=a.add(l.mul(g)).div(u).clamp();return Vs(g,y)},{p1:"vec3",p2:"vec3",p3:"vec3",p4:"vec3",return:"vec2"}),aL=ke(({material:n})=>{let e=n._useDash,t=n._useWorldUnits,i=ts("instanceStart"),s=ts("instanceEnd"),r=gt(bf.mul(gt(i,1))).toVar("start"),o=gt(bf.mul(gt(s,1))).toVar("end"),a,l;e&&(a=it(ts("instanceDistanceStart")).toVar("distanceStart"),l=it(ts("instanceDistanceEnd")).toVar("distanceEnd")),t&&(Zc.assign(r.xyz),Bf.assign(o.xyz));let c=Da.z.div(Da.w),u=Hr.element(2).element(3).equal(-1);if(Wr(u,()=>{Wr(r.z.lessThan(0).and(o.z.greaterThan(0)),()=>{let p=vx({start:r,end:o});o.assign(gt(ji(r.xyz,o.xyz,p),o.w)),e&&l.assign(ji(a,l,p))}).ElseIf(o.z.lessThan(0).and(r.z.greaterThanEqual(0)),()=>{let p=vx({start:o,end:r});r.assign(gt(ji(o.xyz,r.xyz,p),r.w)),e&&a.assign(ji(l,a,p))})}),e){let p=n.dashScaleNode?it(n.dashScaleNode):vC,E=n.offsetNode?it(n.offsetNode):EC,w=ti.y.lessThan(.5).select(p.mul(a),p.mul(l));w=w.add(E),l0.assign(w)}let d=Hr.mul(r),h=Hr.mul(o),f=d.xyz.div(d.w),g=h.xyz.div(h.w),y=g.xy.sub(f.xy).toVar();y.x.assign(y.x.mul(c)),y.assign(y.normalize());let m=gt().toVar();if(t){let p=o.xyz.sub(r.xyz).normalize(),E=ji(r.xyz,o.xyz,.5).normalize(),w=p.cross(E).normalize(),S=p.cross(w);Us.assign(ti.y.lessThan(.5).select(r,o));let v=wf.mul(.5);Us.addAssign(gt(ti.x.lessThan(0).select(w.mul(v),w.mul(v).negate()),0)),e||(Us.addAssign(gt(ti.y.lessThan(.5).select(p.mul(v).negate(),p.mul(v)),0)),Us.addAssign(gt(S.mul(v),0)),Wr(ti.y.greaterThan(1).or(ti.y.lessThan(0)),()=>{Us.subAssign(gt(S.mul(2).mul(v),0))})),m.assign(Hr.mul(Us));let A=wt().toVar();A.assign(ti.y.lessThan(.5).select(f,g)),m.z.assign(A.z.mul(m.w))}else{let p=Vs(y.y,y.x.negate()).toVar("offset");y.x.assign(y.x.div(c)),p.x.assign(p.x.div(c)),p.assign(ti.x.lessThan(0).select(p.negate(),p)),Wr(ti.y.lessThan(0),()=>{p.assign(p.sub(y))}).ElseIf(ti.y.greaterThan(1),()=>{p.assign(p.add(y))}),p.assign(p.mul(wf)),p.assign(p.div(Da.w.div(zw))),m.assign(ti.y.lessThan(.5).select(d,h)),p.assign(p.mul(m.w)),m.assign(m.add(gt(p,0,0)))}return m})(),lL=ke(({material:n,renderer:e})=>{let t=n._useAlphaToCoverage,i=n._useDash,s=n._useWorldUnits,r=gu();if(i){let a=n.dashSizeNode?it(n.dashSizeNode):AC,l=n.gapSizeNode?it(n.gapSizeNode):TC;Rd.assign(a),cx.assign(l),r.y.lessThan(-1).or(r.y.greaterThan(1)).discard(),l0.mod(Rd.add(cx)).greaterThan(Rd).discard()}let o=it(1).toVar("alpha");if(s){let a=Us.xyz.normalize().mul(1e5),l=Bf.sub(Zc),c=IC({p1:Zc,p2:Bf,p3:wt(0,0,0),p4:a}),u=Zc.add(l.mul(c.x)),d=a.mul(c.y),g=u.sub(d).length().div(wf);if(!i)if(t&&e.currentSamples>0){let y=g.fwidth();o.assign(Va(y.negate().add(.5),y.add(.5),g).oneMinus())}else g.greaterThan(.5).discard()}else if(t&&e.currentSamples>0){let a=r.x,l=r.y.greaterThan(0).select(r.y.sub(1),r.y.add(1)),c=a.mul(a).add(l.mul(l)),u=it(c.fwidth()).toVar("dlen");Wr(r.y.abs().greaterThan(1),()=>{o.assign(Va(u.oneMinus(),u.add(1),c).oneMinus())})}else Wr(r.y.abs().greaterThan(1),()=>{let a=r.x,l=r.y.greaterThan(0).select(r.y.sub(1),r.y.add(1));a.mul(a).add(l.mul(l)).greaterThan(1).discard()});return o})();var cL=it(1/Math.PI);var uL=new Uint16Array([12469,15057,12620,14925,13266,14620,13807,14376,14323,13990,14545,13625,14713,13328,14840,12882,14931,12528,14996,12233,15039,11829,15066,11525,15080,11295,15085,10976,15082,10705,15073,10495,13880,14564,13898,14542,13977,14430,14158,14124,14393,13732,14556,13410,14702,12996,14814,12596,14891,12291,14937,11834,14957,11489,14958,11194,14943,10803,14921,10506,14893,10278,14858,9960,14484,14039,14487,14025,14499,13941,14524,13740,14574,13468,14654,13106,14743,12678,14818,12344,14867,11893,14889,11509,14893,11180,14881,10751,14852,10428,14812,10128,14765,9754,14712,9466,14764,13480,14764,13475,14766,13440,14766,13347,14769,13070,14786,12713,14816,12387,14844,11957,14860,11549,14868,11215,14855,10751,14825,10403,14782,10044,14729,9651,14666,9352,14599,9029,14967,12835,14966,12831,14963,12804,14954,12723,14936,12564,14917,12347,14900,11958,14886,11569,14878,11247,14859,10765,14828,10401,14784,10011,14727,9600,14660,9289,14586,8893,14508,8533,15111,12234,15110,12234,15104,12216,15092,12156,15067,12010,15028,11776,14981,11500,14942,11205,14902,10752,14861,10393,14812,9991,14752,9570,14682,9252,14603,8808,14519,8445,14431,8145,15209,11449,15208,11451,15202,11451,15190,11438,15163,11384,15117,11274,15055,10979,14994,10648,14932,10343,14871,9936,14803,9532,14729,9218,14645,8742,14556,8381,14461,8020,14365,7603,15273,10603,15272,10607,15267,10619,15256,10631,15231,10614,15182,10535,15118,10389,15042,10167,14963,9787,14883,9447,14800,9115,14710,8665,14615,8318,14514,7911,14411,7507,14279,7198,15314,9675,15313,9683,15309,9712,15298,9759,15277,9797,15229,9773,15166,9668,15084,9487,14995,9274,14898,8910,14800,8539,14697,8234,14590,7790,14479,7409,14367,7067,14178,6621,15337,8619,15337,8631,15333,8677,15325,8769,15305,8871,15264,8940,15202,8909,15119,8775,15022,8565,14916,8328,14804,8009,14688,7614,14569,7287,14448,6888,14321,6483,14088,6171,15350,7402,15350,7419,15347,7480,15340,7613,15322,7804,15287,7973,15229,8057,15148,8012,15046,7846,14933,7611,14810,7357,14682,7069,14552,6656,14421,6316,14251,5948,14007,5528,15356,5942,15356,5977,15353,6119,15348,6294,15332,6551,15302,6824,15249,7044,15171,7122,15070,7050,14949,6861,14818,6611,14679,6349,14538,6067,14398,5651,14189,5311,13935,4958,15359,4123,15359,4153,15356,4296,15353,4646,15338,5160,15311,5508,15263,5829,15188,6042,15088,6094,14966,6001,14826,5796,14678,5543,14527,5287,14377,4985,14133,4586,13869,4257,15360,1563,15360,1642,15358,2076,15354,2636,15341,3350,15317,4019,15273,4429,15203,4732,15105,4911,14981,4932,14836,4818,14679,4621,14517,4386,14359,4156,14083,3795,13808,3437,15360,122,15360,137,15358,285,15355,636,15344,1274,15322,2177,15281,2765,15215,3223,15120,3451,14995,3569,14846,3567,14681,3466,14511,3305,14344,3121,14037,2800,13753,2467,15360,0,15360,1,15359,21,15355,89,15346,253,15325,479,15287,796,15225,1148,15133,1492,15008,1749,14856,1882,14685,1886,14506,1783,14324,1608,13996,1398,13702,1183]);var hL=1/6;var dL=wt(.04),fL=it(1);var pL=ks("vec3"),mL=ks("vec3"),gL=ks("vec3");var nu=class extends qc{static get type(){return"OverrideContextNode"}constructor(e,t=null){super(t,{overrideNodes:e}),this.isOverrideContextNode=!0}getFlowContextData(){let e=[];this.traverse(s=>{s.isOverrideContextNode===!0&&e.push(s.value.overrideNodes)});let t=new Map(e.flatMap(s=>Array.from(s.entries()))),i=super.getFlowContextData();return i.overrideNodes=t,i}};function BC(n,e=null,t=null){if(e&&e.isNode){let i=e;e=()=>i}return new nu(new Map([[n,e]]),t)}W("overrideNode",(n,e,t)=>BC(e,t,n));function FC(n,e=null){let t=new Map;for(let[i,s]of n){let r=s!==null?typeof s=="function"?s:()=>s:null;t.set(i,r)}return new nu(t,e)}W("overrideNodes",(n,e)=>FC(e,n));var xL=new Rn,yL=new L,_L=new L,bL=new L,SL=new ct,vL=new L(0,0,-1),AL=new dt,TL=new L,EL=new L,ML=new dt,RL=new ze,LC=new dr,wL=ip.flipX();LC.depthTexture=new $n(1,1);var CL=gt(Zd([-1,-1,3]).element(fx),Zd([3,-1,-1]).element(fx),0,1);var NL=Os(wt(1.6605,-.1246,-.0182),wt(-.5876,1.1329,-.1006),wt(-.0728,-.0083,1.1187)),IL=Os(wt(.6274,.0691,.0164),wt(.3293,.9195,.088),wt(.0433,.0113,.8956));var mt=class extends De{static get type(){return"CodeNode"}constructor(e="",t=[],i=""){super("code"),this.isCodeNode=!0,this.global=!0,this.code=e,this.includes=t,this.language=i}setIncludes(e){return this.includes=e,this}getIncludes(){return this.includes}generate(e){let t=this.getIncludes(e);for(let s of t)s.build(e,"void");let i=e.getCodeFromNode(this,this.getNodeType(e));return i.code=this.code,i.code}serialize(e){super.serialize(e),e.code=this.code,e.language=this.language}deserialize(e){super.deserialize(e),this.code=e.code,this.language=e.language}};var BL=ks("vec3","totalDiffuse"),FL=ks("vec3","totalSpecular"),LL=ks("vec3","outgoingLight");var Ff=class{constructor(e,t,i=null,s="",r=!1){this.type=e,this.name=t,this.count=i,this.qualifier=s,this.isConst=r}};Ff.isNodeFunctionInput=!0;var Lf=class{constructor(e,t,i="",s=""){this.type=e,this.inputs=t,this.name=i,this.precision=s}getCode(){pe("Abstract function.")}};Lf.isNodeFunction=!0;var UL={[Jn]:Dt,[Dt]:Jn,[sn]:sn};var DL={textureGather:new mt(`
vec4 tsl_textureGather( const int comp, sampler2D map, vec2 coord, ivec2 offset, bool flipY ) {
	if ( flipY ) offset.y = - offset.y;
	vec2 size = vec2( textureSize( map, 0 ) );
	vec2 st = floor( coord * size + vec2( offset ) - 0.5 );
	vec4 ij = vec4( st + 0.5, st + 1.5 ) / size.xyxy;
	vec4 ret = vec4(
		textureLod( map, ij.xw, 0.0 )[ comp ],
		textureLod( map, ij.zw, 0.0 )[ comp ],
		textureLod( map, ij.zy, 0.0 )[ comp ],
		textureLod( map, ij.xy, 0.0 )[ comp ]
	);
	return flipY ? ret.wzyx : ret;
}
`),textureGatherArray:new mt(`
vec4 tsl_textureGather_array( const int comp, sampler2DArray map, vec3 coord, ivec2 offset, bool flipY ) {
	if ( flipY ) offset.y = - offset.y;
	vec2 size = vec2( textureSize( map, 0 ).xy );
	vec2 st = floor( coord.xy * size + vec2( offset ) - 0.5 );
	vec4 ij = vec4( st + 0.5, st + 1.5 ) / size.xyxy;
	vec4 ret = vec4(
		textureLod( map, vec3( ij.xw, coord.z ), 0.0 )[ comp ],
		textureLod( map, vec3( ij.zw, coord.z ), 0.0 )[ comp ],
		textureLod( map, vec3( ij.zy, coord.z ), 0.0 )[ comp ],
		textureLod( map, vec3( ij.xy, coord.z ), 0.0 )[ comp ]
	);
	return flipY ? ret.wzyx : ret;
}
`),textureGatherCompare:new mt(`
vec4 tsl_textureGatherCompare( sampler2DShadow map, vec2 coord, ivec2 offset, float ref, bool flipY ) {
	if ( flipY ) offset.y = - offset.y;
	vec2 size = vec2( textureSize( map, 0 ) );
	vec2 st = floor( coord * size + vec2( offset ) - 0.5 );
	vec4 ij = vec4( st + 0.5, st + 1.5 ) / size.xyxy;
	vec4 ret = vec4(
		textureLod( map, vec3( ij.xw, ref ), 0.0 ),
		textureLod( map, vec3( ij.zw, ref ), 0.0 ),
		textureLod( map, vec3( ij.zy, ref ), 0.0 ),
		textureLod( map, vec3( ij.xy, ref ), 0.0 )
	);
	return flipY ? ret.wzyx : ret;
}
`),textureGatherCompareArray:new mt(`
vec4 tsl_textureGatherCompare_array( sampler2DArrayShadow map, vec3 coord, ivec2 offset, float ref, bool flipY ) {
	if ( flipY ) offset.y = - offset.y;
	vec2 size = vec2( textureSize( map, 0 ).xy );
	vec2 st = floor( coord.xy * size + vec2( offset ) - 0.5 );
	vec4 ij = vec4( st + 0.5, st + 1.5 ) / size.xyxy;
	vec4 ret = vec4(
		texture( map, vec4( ij.xw, coord.z, ref ) ),
		texture( map, vec4( ij.zw, coord.z, ref ) ),
		texture( map, vec4( ij.zy, coord.z, ref ) ),
		texture( map, vec4( ij.xy, coord.z, ref ) )
	);
	return flipY ? ret.wzyx : ret;
}
`),floatpack_snorm_4x8:new mt("uint tsl_packSnorm4x8( vec4 v ) { uvec4 u = uvec4( ivec4( round( clamp( v, -1.0, 1.0 ) * 127.0 ) ) & 0xff ); return u.x | u.y << 8 | u.z << 16 | u.w << 24; }"),floatpack_unorm_4x8:new mt("uint tsl_packUnorm4x8( vec4 v ) { uvec4 u = uvec4( round( clamp( v, 0.0, 1.0 ) * 255.0 ) ); return u.x | u.y << 8 | u.z << 16 | u.w << 24; }"),floatunpack_snorm_4x8:new mt("vec4 tsl_unpackSnorm4x8( uint x ) { return max( vec4( ivec4( x << 24, x << 16, x << 8, x ) >> 24 ) / 127.0, -1.0 ); }"),floatunpack_unorm_4x8:new mt("vec4 tsl_unpackUnorm4x8( uint x ) { return vec4( uvec4( x, x >> 8, x >> 16, x >> 24 ) & 0xffu ) / 255.0; }")};var Ud=typeof self<"u"&&self.GPUShaderStage?self.GPUShaderStage:{VERTEX:1,FRAGMENT:2,COMPUTE:4};var iu=class{constructor(){this.label="",this.layout=null,this.entries=[]}reset(){this.label="",this.layout=null,this.entries.length=0}},ns=class{constructor(){this.label="",this.size=0,this.usage=0,this.mappedAtCreation=!1}reset(){this.label="",this.size=0,this.usage=0,this.mappedAtCreation=!1}},Gs=class{constructor(){this.label=""}reset(){this.label=""}},su=class{constructor(){this.label="",this.colorFormats=null,this.depthStencilFormat=void 0,this.sampleCount=1,this.depthReadOnly=!1,this.stencilReadOnly=!1}reset(){this.label="",this.colorFormats=null,this.depthStencilFormat=void 0,this.sampleCount=1,this.depthReadOnly=!1,this.stencilReadOnly=!1}},Uf=class{constructor(){this.view=null,this.depthSlice=void 0,this.resolveTarget=void 0,this.clearValue=void 0,this.loadOp=void 0,this.storeOp=void 0}reset(){this.view=null,this.depthSlice=void 0,this.resolveTarget=void 0,this.clearValue=void 0,this.loadOp=void 0,this.storeOp=void 0}},Df=class{constructor(){this.label="",this.colorAttachments=[],this.depthStencilAttachment=void 0,this.occlusionQuerySet=void 0,this.timestampWrites=void 0,this.maxDrawCount=5e7}reset(){this.label="",this.colorAttachments.length=0,this.depthStencilAttachment=void 0,this.occlusionQuerySet=void 0,this.timestampWrites=void 0,this.maxDrawCount=5e7}},ru=class{constructor(){this.label="",this.layout=null,this.vertex=null,this.primitive={},this.depthStencil=void 0,this.multisample=new Pf,this.fragment=null}reset(){this.label="",this.layout=null,this.vertex=null,this.primitive={},this.depthStencil=void 0,this.multisample.reset(),this.fragment=null}},Pf=class{constructor(){this.count=1,this.mask=4294967295,this.alphaToCoverageEnabled=!1}reset(){this.count=1,this.mask=4294967295,this.alphaToCoverageEnabled=!1}},ou=class{constructor(){this.label="",this.code="",this.compilationHints=[]}reset(){this.label="",this.code="",this.compilationHints.length=0}},za=class{constructor(){this.label="",this.size={width:0,height:1,depthOrArrayLayers:1},this.mipLevelCount=1,this.sampleCount=1,this.dimension="2d",this.format=void 0,this.usage=void 0,this.viewFormats=[],this.textureBindingViewDimension=void 0}reset(){this.label="",this.size.width=0,this.size.height=1,this.size.depthOrArrayLayers=1,this.mipLevelCount=1,this.sampleCount=1,this.dimension="2d",this.format=void 0,this.usage=void 0,this.viewFormats.length=0,this.textureBindingViewDimension=void 0}},Wa=class{constructor(){this.label="",this.format=void 0,this.dimension=void 0,this.usage=0,this.aspect="all",this.baseMipLevel=0,this.mipLevelCount=void 0,this.baseArrayLayer=0,this.arrayLayerCount=void 0,this.swizzle="rgba"}reset(){this.label="",this.format=void 0,this.dimension=void 0,this.usage=0,this.aspect="all",this.baseMipLevel=0,this.mipLevelCount=void 0,this.baseArrayLayer=0,this.arrayLayerCount=void 0,this.swizzle="rgba"}},PL=new iu,GL=new ns,VL=new Gs,OL=new su,kL=new Df,zL=new ru,WL=new Uf,HL=new ou,YL=new za,ZL=new Wa;var Gf=class{constructor(){this.label="",this.addressModeU="clamp-to-edge",this.addressModeV="clamp-to-edge",this.addressModeW="clamp-to-edge",this.magFilter="nearest",this.minFilter="nearest",this.mipmapFilter="nearest",this.lodMinClamp=0,this.lodMaxClamp=32,this.compare=void 0,this.maxAnisotropy=1}reset(){this.label="",this.addressModeU="clamp-to-edge",this.addressModeV="clamp-to-edge",this.addressModeW="clamp-to-edge",this.magFilter="nearest",this.minFilter="nearest",this.mipmapFilter="nearest",this.lodMinClamp=0,this.lodMaxClamp=32,this.compare=void 0,this.maxAnisotropy=1}},Xr=class{constructor(){this.texture=null,this.mipLevel=0,this.origin={x:0,y:0,z:0},this.aspect="all"}reset(){this.texture=null,this.mipLevel=0,this.origin.x=0,this.origin.y=0,this.origin.z=0,this.aspect="all"}},Vf=class{constructor(){this.buffer=null,this.offset=0,this.bytesPerRow=void 0,this.rowsPerImage=void 0}reset(){this.buffer=null,this.offset=0,this.bytesPerRow=void 0,this.rowsPerImage=void 0}},Of=class{constructor(){this.offset=0,this.bytesPerRow=void 0,this.rowsPerImage=void 0}reset(){this.offset=0,this.bytesPerRow=void 0,this.rowsPerImage=void 0}},kf=class{constructor(){this.source=null,this.origin={x:0,y:0},this.flipY=!1}reset(){this.source=null,this.origin.x=0,this.origin.y=0,this.flipY=!1}},zf=class extends Xr{constructor(){super(),this.colorSpace="srgb",this.premultipliedAlpha=!1}reset(){super.reset(),this.colorSpace="srgb",this.premultipliedAlpha=!1}},au=class{constructor(){this.width=0,this.height=1,this.depthOrArrayLayers=1}reset(){this.width=0,this.height=1,this.depthOrArrayLayers=1}},XL=new ns,KL=new Gs,$L=new Gf,JL=new Xr,qL=new Vf,QL=new Of,jL=new kf,eU=new zf,tU=new za,nU=new au,iU={[lc]:"never",[ma]:"less",[cc]:"equal",[ws]:"less-equal",[Fr]:"greater",[$i]:"greater-equal",[hc]:"always",[uc]:"not-equal"};var sU={[Md.READ_ONLY]:"read",[Md.WRITE_ONLY]:"write",[Md.READ_WRITE]:"read_write"},rU={[ar]:"repeat",[Ln]:"clamp",[lr]:"mirror"},oU={vertex:Ud.VERTEX,fragment:Ud.FRAGMENT,compute:Ud.COMPUTE};var aU={tsl_xor:new mt("fn tsl_xor( a : bool, b : bool ) -> bool { return ( a || b ) && !( a && b ); }"),mod_float:new mt("fn tsl_mod_float( x : f32, y : f32 ) -> f32 { return x - y * floor( x / y ); }"),mod_vec2:new mt("fn tsl_mod_vec2( x : vec2f, y : vec2f ) -> vec2f { return x - y * floor( x / y ); }"),mod_vec3:new mt("fn tsl_mod_vec3( x : vec3f, y : vec3f ) -> vec3f { return x - y * floor( x / y ); }"),mod_vec4:new mt("fn tsl_mod_vec4( x : vec4f, y : vec4f ) -> vec4f { return x - y * floor( x / y ); }"),equals_bool:new mt("fn tsl_equals_bool( a : bool, b : bool ) -> bool { return a == b; }"),equals_bvec2:new mt("fn tsl_equals_bvec2( a : vec2f, b : vec2f ) -> vec2<bool> { return vec2<bool>( a.x == b.x, a.y == b.y ); }"),equals_bvec3:new mt("fn tsl_equals_bvec3( a : vec3f, b : vec3f ) -> vec3<bool> { return vec3<bool>( a.x == b.x, a.y == b.y, a.z == b.z ); }"),equals_bvec4:new mt("fn tsl_equals_bvec4( a : vec4f, b : vec4f ) -> vec4<bool> { return vec4<bool>( a.x == b.x, a.y == b.y, a.z == b.z, a.w == b.w ); }"),repeatWrapping_float:new mt("fn tsl_repeatWrapping_float( coord: f32 ) -> f32 { return fract( coord ); }"),mirrorWrapping_float:new mt("fn tsl_mirrorWrapping_float( coord: f32 ) -> f32 { let mirrored = fract( coord * 0.5 ) * 2.0; return 1.0 - abs( 1.0 - mirrored ); }"),clampWrapping_float:new mt("fn tsl_clampWrapping_float( coord: f32 ) -> f32 { return clamp( coord, 0.0, 1.0 ); }"),inverse_mat2:new mt(`
fn tsl_inverse_mat2( m : mat2x2<f32> ) -> mat2x2<f32> {

	let det = m[ 0 ][ 0 ] * m[ 1 ][ 1 ] - m[ 0 ][ 1 ] * m[ 1 ][ 0 ];

	return mat2x2<f32>(
		m[ 1 ][ 1 ], - m[ 0 ][ 1 ],
		- m[ 1 ][ 0 ], m[ 0 ][ 0 ]
	) * ( 1.0 / det );

}
`),inverse_mat3:new mt(`
fn tsl_inverse_mat3( m : mat3x3<f32> ) -> mat3x3<f32> {

	let a00 = m[ 0 ][ 0 ]; let a01 = m[ 0 ][ 1 ]; let a02 = m[ 0 ][ 2 ];
	let a10 = m[ 1 ][ 0 ]; let a11 = m[ 1 ][ 1 ]; let a12 = m[ 1 ][ 2 ];
	let a20 = m[ 2 ][ 0 ]; let a21 = m[ 2 ][ 1 ]; let a22 = m[ 2 ][ 2 ];

	let b01 = a22 * a11 - a12 * a21;
	let b11 = - a22 * a10 + a12 * a20;
	let b21 = a21 * a10 - a11 * a20;

	let det = a00 * b01 + a01 * b11 + a02 * b21;

	return mat3x3<f32>(
		b01, ( - a22 * a01 + a02 * a21 ), ( a12 * a01 - a02 * a11 ),
		b11, ( a22 * a00 - a02 * a20 ), ( - a12 * a00 + a02 * a10 ),
		b21, ( - a21 * a00 + a01 * a20 ), ( a11 * a00 - a01 * a10 )
	) * ( 1.0 / det );

}
`),inverse_mat4:new mt(`
fn tsl_inverse_mat4( m : mat4x4<f32> ) -> mat4x4<f32> {

	let a00 = m[ 0 ][ 0 ]; let a01 = m[ 0 ][ 1 ]; let a02 = m[ 0 ][ 2 ]; let a03 = m[ 0 ][ 3 ];
	let a10 = m[ 1 ][ 0 ]; let a11 = m[ 1 ][ 1 ]; let a12 = m[ 1 ][ 2 ]; let a13 = m[ 1 ][ 3 ];
	let a20 = m[ 2 ][ 0 ]; let a21 = m[ 2 ][ 1 ]; let a22 = m[ 2 ][ 2 ]; let a23 = m[ 2 ][ 3 ];
	let a30 = m[ 3 ][ 0 ]; let a31 = m[ 3 ][ 1 ]; let a32 = m[ 3 ][ 2 ]; let a33 = m[ 3 ][ 3 ];

	let b00 = a00 * a11 - a01 * a10;
	let b01 = a00 * a12 - a02 * a10;
	let b02 = a00 * a13 - a03 * a10;
	let b03 = a01 * a12 - a02 * a11;
	let b04 = a01 * a13 - a03 * a11;
	let b05 = a02 * a13 - a03 * a12;
	let b06 = a20 * a31 - a21 * a30;
	let b07 = a20 * a32 - a22 * a30;
	let b08 = a20 * a33 - a23 * a30;
	let b09 = a21 * a32 - a22 * a31;
	let b10 = a21 * a33 - a23 * a31;
	let b11 = a22 * a33 - a23 * a32;

	let det = b00 * b11 - b01 * b10 + b02 * b09 + b03 * b08 - b04 * b07 + b05 * b06;

	return mat4x4<f32>(
		a11 * b11 - a12 * b10 + a13 * b09,
		a02 * b10 - a01 * b11 - a03 * b09,
		a31 * b05 - a32 * b04 + a33 * b03,
		a22 * b04 - a21 * b05 - a23 * b03,
		a12 * b08 - a10 * b11 - a13 * b07,
		a00 * b11 - a02 * b08 + a03 * b07,
		a32 * b02 - a30 * b05 - a33 * b01,
		a20 * b05 - a22 * b02 + a23 * b01,
		a10 * b10 - a11 * b08 + a13 * b06,
		a01 * b08 - a00 * b10 - a03 * b06,
		a30 * b04 - a31 * b02 + a33 * b00,
		a21 * b02 - a20 * b04 - a23 * b00,
		a11 * b07 - a10 * b09 - a12 * b06,
		a00 * b09 - a01 * b07 + a02 * b06,
		a31 * b01 - a30 * b03 - a32 * b00,
		a20 * b03 - a21 * b01 + a22 * b00
	) * ( 1.0 / det );

}
`),biquadraticTexture:new mt(`
fn tsl_biquadraticTexture( map : texture_2d<f32>, coord : vec2f, iRes : vec2u, level : u32 ) -> vec4f {

	let res = vec2f( iRes );

	let uvScaled = coord * res;
	let uvWrapping = ( ( uvScaled % res ) + res ) % res;

	// https://www.shadertoy.com/view/WtyXRy

	let uv = uvWrapping - 0.5;
	let iuv = floor( uv );
	let f = fract( uv );

	let rg1 = textureLoad( map, vec2u( iuv + vec2( 0.5, 0.5 ) ) % iRes, level );
	let rg2 = textureLoad( map, vec2u( iuv + vec2( 1.5, 0.5 ) ) % iRes, level );
	let rg3 = textureLoad( map, vec2u( iuv + vec2( 0.5, 1.5 ) ) % iRes, level );
	let rg4 = textureLoad( map, vec2u( iuv + vec2( 1.5, 1.5 ) ) % iRes, level );

	return mix( mix( rg1, rg2, f.x ), mix( rg3, rg4, f.x ), f.y );

}
`),biquadraticTextureArray:new mt(`
fn tsl_biquadraticTexture_array( map : texture_2d_array<f32>, coord : vec2f, iRes : vec2u, layer : u32, level : u32 ) -> vec4f {

	let res = vec2f( iRes );

	let uvScaled = coord * res;
	let uvWrapping = ( ( uvScaled % res ) + res ) % res;

	// https://www.shadertoy.com/view/WtyXRy

	let uv = uvWrapping - 0.5;
	let iuv = floor( uv );
	let f = fract( uv );

	let rg1 = textureLoad( map, vec2u( iuv + vec2( 0.5, 0.5 ) ) % iRes, layer, level );
	let rg2 = textureLoad( map, vec2u( iuv + vec2( 1.5, 0.5 ) ) % iRes, layer, level );
	let rg3 = textureLoad( map, vec2u( iuv + vec2( 0.5, 1.5 ) ) % iRes, layer, level );
	let rg4 = textureLoad( map, vec2u( iuv + vec2( 1.5, 1.5 ) ) % iRes, layer, level );

	return mix( mix( rg1, rg2, f.x ), mix( rg3, rg4, f.x ), f.y );

}
`)};var UC="";(typeof navigator<"u"&&/Firefox|Deno/g.test(navigator.userAgent))!==!0&&(UC+=`diagnostic( off, derivative_uniformity );
`);var lU=new ns,cU=new Gs,DC=new Map([[Int8Array,["sint8","snorm8"]],[Uint8Array,["uint8","unorm8"]],[Int16Array,["sint16","snorm16"]],[Uint16Array,["uint16","unorm16"]],[Int32Array,["sint32"]],[Uint32Array,["uint32"]],[Float32Array,["float32"]]]);typeof Float16Array<"u"&&DC.set(Float16Array,["float16"]);var uU=new iu,hU=new ns,dU=new Wa;var Wf=class{constructor(){this.label="",this.layout=null,this.compute=null}reset(){this.label="",this.layout=null,this.compute=null}},Hf=class{constructor(){this.label="",this.bindGroupLayouts=null}reset(){this.label="",this.bindGroupLayouts=null}},fU=new Wf,pU=new Hf,mU=new su,gU=new ru;var lu=class{constructor(){this.label="",this.type=void 0,this.count=0}reset(){this.label="",this.type=void 0,this.count=0}},xU=new ns,yU=new Gs,_U=new lu;var Yf=class{constructor(){this.label="",this.timestampWrites=void 0}reset(){this.label="",this.timestampWrites=void 0}};var Zf=class{constructor(){this.querySet=null,this.beginningOfPassWriteIndex=void 0,this.endOfPassWriteIndex=void 0}reset(){this.querySet=null,this.beginningOfPassWriteIndex=void 0,this.endOfPassWriteIndex=void 0}};var bU=new ns,SU=new Gs,vU=new Yf,AU=new lu,TU=new ou,EU=new Zf,MU=new Xr,RU=new Xr,wU=new za,CU=new Wa,NU=new au;var _u=class{element;target;opts;wheelMode="auto";pointers=new Map;dragMode="none";lastX=0;lastY=0;downX=0;downY=0;downAt=0;downButton=-1;moved=!1;caught=!1;lastMoveAt=0;pinchDist=0;pinchMidX=0;pinchMidY=0;pairAngle=0;pairRot=0;pairScale=1;pairEngaged=!1;pairCancelAt=0;longPressTimer=null;lastTapAt=0;lastTapX=0;lastTapY=0;wheelStream=null;pinchSigns=[];gesture=null;contextAt=null;contextPending=null;listeners;constructor(e,t,i={}){this.element=e,this.target=t,this.opts=i,e.style.touchAction="none",e.style.userSelect="none",e.style.setProperty("-webkit-user-select","none"),e.style.setProperty("-webkit-touch-callout","none");let s=r=>r.preventDefault();this.listeners=[["pointerdown",this.onPointerDown],["pointermove",this.onPointerMove],["pointerup",this.onPointerUp],["pointercancel",this.onPointerUp],["wheel",this.onWheel,{passive:!1}],["gesturestart",this.onGestureEvent,{passive:!1}],["gesturechange",this.onGestureEvent,{passive:!1}],["gestureend",this.onGestureEvent,{passive:!1}],["dblclick",this.onDblClick],["selectstart",s],["contextmenu",this.onContextMenu]];for(let[r,o,a]of this.listeners)e.addEventListener(r,o,a)}dispose(){this.cancelLongPress(),this.dropPendingContext();for(let[e,t]of this.listeners)this.element.removeEventListener(e,t)}local(e){let t=this.element.getBoundingClientRect();return{x:e.clientX-t.left,y:e.clientY-t.top}}pair(){let e=this.pointers.values(),t=e.next().value??{x:0,y:0},i=e.next().value??t;return[t,i]}onPointerDown=e=>{let t=this.local(e);this.pointers.set(e.pointerId,{x:t.x,y:t.y,type:e.pointerType});try{this.element.setPointerCapture(e.pointerId)}catch{}let i=performance.now();if(this.dropPendingContext(),this.pointers.size===1&&(this.caught=this.target.coasting(),this.target.active(!0)),this.target.stop(),this.pointers.size===1){this.downX=this.lastX=t.x,this.downY=this.lastY=t.y,this.downAt=i,this.downButton=e.button,this.moved=!1;let s=this.contextAt;s&&e.button===0&&i-s.t<1500&&Math.hypot(t.x-s.x,t.y-s.y)<24&&(this.contextAt=null,this.target.contextDismiss?.()),this.gesture?.touch||e.pointerType==="mouse"&&e.button===0&&i-this.pairCancelAt<300?this.dragMode="pan":e.pointerType==="touch"?(this.dragMode="primary",this.longPressTimer=window.setTimeout(()=>{this.longPressTimer=null,!this.moved&&this.pointers.size===1&&(this.dragMode="none",this.openContext(t.x,t.y))},500)):e.button===0&&!e.shiftKey?this.dragMode="primary":(e.button===0||e.button===1||e.button===2)&&(this.dragMode="pan"),this.dragMode!=="none"&&this.target.dragStart(this.dragMode)}else if(this.pointers.size===2){this.cancelLongPress(),this.dragMode="none";let[s,r]=this.pair();this.pinchDist=Math.hypot(s.x-r.x,s.y-r.y),this.pinchMidX=(s.x+r.x)/2,this.pinchMidY=(s.y+r.y)/2,this.pairAngle=Math.atan2(r.y-s.y,r.x-s.x),this.pairRot=0,this.pairScale=1,this.pairEngaged=!1}};onPointerMove=e=>{if(!this.pointers.has(e.pointerId))return;let t=this.local(e);if(this.pointers.set(e.pointerId,{x:t.x,y:t.y,type:e.pointerType}),this.pointers.size===2){let[o,a]=this.pair(),l=Math.hypot(o.x-a.x,o.y-a.y),c=(o.x+a.x)/2,u=(o.y+a.y)/2;this.pinchDist>0&&l>0&&(this.target.zoomAt(c,u,this.pinchDist/l),this.pairScale*=l/this.pinchDist),this.target.pan(c-this.pinchMidX,u-this.pinchMidY);let d=Math.atan2(a.y-o.y,a.x-o.x),h=d-this.pairAngle;for(;h>Math.PI;)h-=2*Math.PI;for(;h<-Math.PI;)h+=2*Math.PI;this.pairAngle=d,this.pairRot+=h,this.opts.twist&&this.target.twistAt&&(!this.pairEngaged&&u0(this.pairRot*180/Math.PI,this.pairScale)&&(this.pairEngaged=!0,h=0),this.pairEngaged&&h!==0&&this.target.twistAt(c,u,h)),this.pinchDist=l,this.pinchMidX=c,this.pinchMidY=u,this.moved=!0;return}let i=t.x-this.lastX,s=t.y-this.lastY,r=e.pointerType==="touch";!this.moved&&Math.hypot(t.x-this.downX,t.y-this.downY)>(r?10:5)&&(r&&(i=s=0),this.moved=!0,this.cancelLongPress(),this.dropPendingContext()),!(!this.moved||this.dragMode==="none")&&(this.lastMoveAt=performance.now(),this.dragMode==="primary"?this.target.primary(i,s):this.target.pan(i,s),this.lastX=t.x,this.lastY=t.y)};onPointerUp=e=>{if(!this.pointers.has(e.pointerId))return;let t=performance.now();e.type==="pointercancel"&&e.pointerType==="mouse"&&!this.moved&&t-this.downAt<150&&(this.pairCancelAt=t);let i=this.local(e);if(this.pointers.delete(e.pointerId),this.cancelLongPress(),this.element.hasPointerCapture(e.pointerId)&&this.element.releasePointerCapture(e.pointerId),this.pointers.size>0){let a=[...this.pointers.values()][0];this.lastX=a.x,this.lastY=a.y,this.dragMode="primary",this.target.dragStart("primary"),this.moved=!0;return}this.gesture||this.target.active(!1);let s=this.moved,r=this.dragMode;if(this.dragMode="none",s){r!=="none"&&this.target.dragEnd(r,t-this.lastMoveAt<80);return}if(e.type==="pointercancel"||t-this.downAt>700)return;if(e.pointerType==="touch"){if(this.caught)return;if(t-this.lastTapAt<320&&Math.hypot(i.x-this.lastTapX,i.y-this.lastTapY)<30){this.lastTapAt=0,this.target.doubleTap(i.x,i.y);return}this.lastTapAt=t,this.lastTapX=i.x,this.lastTapY=i.y,this.target.tap(i.x,i.y,!1);return}let o=this.contextPending;if(o){this.dropPendingContext(),this.openContext(o.x,o.y);return}this.downButton===0&&this.target.tap(i.x,i.y,e.shiftKey||e.ctrlKey||e.metaKey)};onDblClick=e=>{e.preventDefault();let t=this.local(e);this.target.doubleTap(t.x,t.y)};onContextMenu=e=>{e.preventDefault();let t=this.local(e);if(this.pointers.size===1&&this.downButton===2&&!this.moved){this.dropPendingContext();let i=window.setTimeout(()=>{let s=this.contextPending;this.contextPending=null,s&&!this.moved&&this.openContext(s.x,s.y)},400);this.contextPending={x:t.x,y:t.y,timer:i};return}this.openContext(t.x,t.y)};dropPendingContext(){this.contextPending&&window.clearTimeout(this.contextPending.timer),this.contextPending=null}openContext(e,t){let i=performance.now(),s=this.contextAt;s&&i-s.t<600&&Math.hypot(e-s.x,t-s.y)<24||(this.contextAt={x:e,y:t,t:i},this.target.context(e,t))}cancelLongPress(){this.longPressTimer!==null&&(window.clearTimeout(this.longPressTimer),this.longPressTimer=null)}classify(e){if(e.ctrlKey)return"pinch";if(this.wheelMode==="orbit")return"trackpad";if(this.wheelMode==="zoom")return"mouse";let t=performance.now(),i=this.wheelStream;if(i&&t-i.at<300)return i.at=t,i.kind==="mouse"&&!c0(e)&&(i.kind="trackpad"),i.kind;let s=c0(e)?"mouse":"trackpad";return this.wheelStream={kind:s,at:t},s}pinchJitter(e){let t=performance.now(),i=this.pinchSigns;for(;i.length&&t-i[0].at>300;)i.shift();if(e!==0&&i.push({s:Math.sign(e),at:t}),i.length>8&&i.shift(),i.length<4)return!1;let s=0;for(let r=1;r<i.length;r++)i[r].s!==i[r-1].s&&s++;return s*2>i.length-1}onWheel=e=>{e.preventDefault(),this.target.stop();let t=this.local(e),i=this.classify(e);if(i==="pinch"){if(this.gesture||this.pinchJitter(e.deltaY))return;this.target.zoomAt(t.x,t.y,Math.exp(Math.max(-40,Math.min(40,e.deltaY))*.01));return}if(e.shiftKey&&this.target.shiftScroll){this.target.shiftScroll(-e.deltaX*.9,-e.deltaY*.9);return}if(i==="mouse"){let s=e.deltaMode===WheelEvent.DOM_DELTA_PIXEL?e.deltaY:e.deltaY*40;this.target.zoomAt(t.x,t.y,Math.exp(Math.max(-60,Math.min(60,s))*.0022));return}this.target.pan(-e.deltaX,-e.deltaY)};onGestureEvent=e=>{let t=e;t.preventDefault();let i=typeof t.scale=="number"&&t.scale>0?t.scale:1,s=typeof t.rotation=="number"?t.rotation:0;if(t.type==="gesturestart"){this.target.stop();let a=this.pointers.size>0||performance.now()-this.pairCancelAt<300;this.gesture={scale:i,rot:s,rot0:s,scale0:i,engaged:!1,touch:a},a&&this.pointers.size>0&&(this.dragMode!=="pan"&&this.target.dragStart("pan"),this.dragMode="pan",this.moved=!0,this.cancelLongPress()),this.pointers.size===0&&this.target.active(!0);return}let r=this.gesture;if(!r)return;if(t.type==="gestureend"){this.pointers.size===0&&this.target.active(!1),this.gesture=null;return}let o=this.local({clientX:t.clientX??0,clientY:t.clientY??0});i!==r.scale&&this.target.zoomAt(o.x,o.y,r.scale/i),this.opts.twist&&this.target.twistAt&&(!r.engaged&&u0(s-r.rot0,i/r.scale0)&&(r.engaged=!0,r.rot=s),r.engaged&&s!==r.rot&&this.target.twistAt(o.x,o.y,(s-r.rot)*Math.PI/180)),r.scale=i,r.rot=s}};function c0(n){if(n.momentum)return!1;if(n.deltaMode!==0)return!0;let e=Math.abs(n.deltaY);return n.deltaX!==0||e===0||!Number.isInteger(e)?!1:e>=100||e%40===0}function u0(n,e){let t=Math.abs(n);return t>=10&&t/45>Math.abs(Math.log(Math.max(1e-6,e)))}function PC(n,e,t,i,s){if(i<=0||s<=0)return t.v=0,e;let r=2/i,o=r*s,a=1/(1+o+.48*o*o+.235*o*o*o),l=n-e,c=(t.v+r*l)*s;t.v=(t.v-r*c)*a;let u=e+(l+c)*a;return e-n>0==u>e&&(u=e,t.v=0),u}var is=class{value;goal;vel={v:0};drift=0;constructor(e){this.value=e,this.goal=e}step(e,t,i){return this.drift!==0&&(this.goal+=this.drift*t,this.drift*=Math.exp(-t/i),Math.abs(this.drift)<1e-4&&(this.drift=0)),this.value=PC(this.value,this.goal,this.vel,e,t),Math.abs(this.value-this.goal)>1e-6||this.drift!==0}snap(e){this.value=e,this.goal=e,this.vel.v=0,this.drift=0}},$r=5e-4,GC=1.5,VC=.01,OC=60,bu=class{camera;element;callbacks;rotateSpeed=1;theta;phi;logDist;tx;ty;tz;smoothTime=.09;driftTau=.32;disposed=!1;lastStep=0;onGesture=null;velTheta=0;velPhi=0;velTx=0;velTy=0;velTz=0;lastMoveAt=0;bounds=null;reduced;input;centre={x:0,y:0};right=new L;up=new L;tmp=new L;constructor(e,t,i){this.camera=e,this.element=t,this.callbacks=i,this.theta=new is(Math.atan2(-.95,1)),this.phi=new is(Math.acos(.8/Math.hypot(1,.95,.8))),this.logDist=new is(Math.log(250)),this.tx=new is(0),this.ty=new is(0),this.tz=new is(10),this.reduced=window.matchMedia("(prefers-reduced-motion: reduce)"),this.input=new _u(t,this.gestures(),{twist:!0}),this.applyPose()}get wheelMode(){return this.input.wheelMode}set wheelMode(e){this.input.wheelMode=e}get target(){return this.tmp.set(this.tx.goal,this.ty.goal,this.tz.goal)}get distance(){return Math.exp(this.logDist.goal)}pose(){return{theta:this.theta.goal,phi:this.phi.goal,distance:Math.exp(this.logDist.goal),target:{x:this.tx.goal,y:this.ty.goal,z:this.tz.goal}}}exactPose(){return{theta:this.theta.goal,phi:this.phi.goal,logDist:this.logDist.goal,tx:this.tx.goal,ty:this.ty.goal,tz:this.tz.goal}}restoreExact(e){this.theta.snap(e.theta),this.phi.snap(e.phi),this.logDist.snap(e.logDist),this.tx.snap(e.tx),this.ty.snap(e.ty),this.tz.snap(e.tz),this.callbacks.onChange()}get currentDistance(){return Math.exp(this.logDist.value)}currentTarget(e){return e.set(this.tx.value,this.ty.value,this.tz.value)}getPose(){return{theta:this.theta.goal,phi:this.phi.goal,distance:Math.exp(this.logDist.goal),target:new L(this.tx.goal,this.ty.goal,this.tz.goal)}}setPose(e,t=!0){let i=!t||this.reduced.matches,s=(r,o)=>{o!==void 0&&(i?r.snap(o):(r.goal=o,r.drift=0))};if(e.theta!==void 0){let r=e.theta,o=this.theta.value;for(;r-o>Math.PI;)r-=2*Math.PI;for(;r-o<-Math.PI;)r+=2*Math.PI;s(this.theta,r)}s(this.phi,e.phi===void 0?void 0:Math.min(Math.PI-$r,Math.max($r,e.phi))),s(this.logDist,e.distance===void 0?void 0:Math.log(Math.max(.001,e.distance))),e.target&&(s(this.tx,e.target.x),s(this.ty,e.target.y),s(this.tz,e.target.z)),i||(this.smoothTime=.22),this.callbacks.onChange()}setBounds(e,t=0){this.bounds=e&&t>0?{x:e.x,y:e.y,z:e.z,r:t}:null}clampGoal(){let e=this.bounds;if(!e)return;let t=this.tx.goal-e.x,i=this.ty.goal-e.y,s=this.tz.goal-e.z,r=Math.hypot(t,i,s),o=e.r*GC;if(r>o){let c=o/r;this.tx.goal=e.x+t*c,this.ty.goal=e.y+i*c,this.tz.goal=e.z+s*c;for(let u of[this.tx,this.ty,this.tz])u.drift=0}let a=Math.log(e.r*VC),l=Math.log(e.r*OC);this.logDist.goal<a&&(this.logDist.goal=a),this.logDist.goal>l&&(this.logDist.goal=l)}focus(e){this.setPose({target:e},!0)}zoomAt(e,t,i){let s=this.element.getBoundingClientRect(),r=Math.max(1,s.width),o=Math.max(1,s.height),a=(e-this.centre.x)/r*2-1,l=-((t-this.centre.y)/o)*2+1,c=Math.exp(this.logDist.goal),u=c*Math.tan(this.camera.fov*Math.PI/360),d=a*u*(r/o),h=l*u;this.right.setFromMatrixColumn(this.camera.matrixWorld,0),this.up.setFromMatrixColumn(this.camera.matrixWorld,1);let f=1-i;this.tx.goal+=(this.right.x*d+this.up.x*h)*f,this.ty.goal+=(this.right.y*d+this.up.y*h)*f,this.tz.goal+=(this.right.z*d+this.up.z*h)*f,this.logDist.goal=Math.log(Math.max(.001,c*i)),this.clampGoal(),this.smoothTime=.09,this.callbacks.onChange()}twistAt(e,t,i){let s=this.element.getBoundingClientRect(),r=Math.max(1,s.width),o=Math.max(1,s.height);this.tmp.set((e-this.centre.x)/r*2-1,-((t-this.centre.y)/o)*2+1,.5).unproject(this.camera);let a=this.camera.position,l=this.right.copy(this.tmp).sub(a),c=this.tx.goal,u=this.ty.goal;if(Math.abs(l.z)>1e-9){let y=(this.tz.goal-a.z)/l.z;y>0&&Number.isFinite(y)&&(c=a.x+l.x*y,u=a.y+l.y*y)}let d=Math.cos(i),h=Math.sin(i),f=this.tx.goal-c,g=this.ty.goal-u;this.tx.goal=c+f*d-g*h,this.ty.goal=u+f*h+g*d,this.theta.goal+=i,this.clampGoal(),this.smoothTime=.09,this.callbacks.onChange()}orbitBy(e,t){let i=2*Math.PI/Math.max(1,this.element.clientHeight)*this.rotateSpeed;this.theta.goal-=e*i,this.phi.goal=Math.min(Math.PI-$r,Math.max($r,this.phi.goal-t*i)),this.smoothTime=.09,this.callbacks.onChange()}panBy(e,t){let s=2*Math.exp(this.logDist.goal)*Math.tan(this.camera.fov*Math.PI/360)/Math.max(1,this.element.clientHeight);this.right.setFromMatrixColumn(this.camera.matrixWorld,0),this.up.setFromMatrixColumn(this.camera.matrixWorld,1),this.tx.goal+=-this.right.x*e*s+this.up.x*t*s,this.ty.goal+=-this.right.y*e*s+this.up.y*t*s,this.tz.goal+=-this.right.z*e*s+this.up.z*t*s,this.clampGoal(),this.smoothTime=.09,this.callbacks.onChange()}update(e){if(this.disposed)return!1;let t=this.lastStep?Math.min(.05,(e-this.lastStep)/1e3):.016;this.lastStep=e;let i=this.reduced.matches?0:this.smoothTime;(this.tx.drift!==0||this.ty.drift!==0||this.tz.drift!==0||this.logDist.drift!==0)&&this.clampGoal();let s=!1;return s=this.theta.step(i,t,this.driftTau)||s,s=this.phi.step(i,t,this.driftTau)||s,this.phi.value=Math.min(Math.PI-$r,Math.max($r,this.phi.value)),s=this.logDist.step(i,t,this.driftTau)||s,s=this.tx.step(i,t,this.driftTau)||s,s=this.ty.step(i,t,this.driftTau)||s,s=this.tz.step(i,t,this.driftTau)||s,this.applyPose(),s||(this.lastStep=0),s}applyPose(){let e=Math.exp(this.logDist.value),t=Math.sin(this.phi.value);this.camera.position.set(this.tx.value+e*t*Math.cos(this.theta.value),this.ty.value+e*t*Math.sin(this.theta.value),this.tz.value+e*Math.cos(this.phi.value)),this.tmp.set(this.tx.value,this.ty.value,this.tz.value),this.camera.up.set(0,0,1),this.camera.lookAt(this.tmp),this.camera.updateMatrixWorld()}gestures(){let e=t=>{let i=performance.now(),s=Math.max(1,i-this.lastMoveAt)/1e3;this.lastMoveAt=i;let r=this.theta.goal,o=this.phi.goal,a=this.tx.goal,l=this.ty.goal,c=this.tz.goal;t();let u=.5;this.velTheta=this.velTheta*(1-u)+(this.theta.goal-r)/s*u,this.velPhi=this.velPhi*(1-u)+(this.phi.goal-o)/s*u,this.velTx=this.velTx*(1-u)+(this.tx.goal-a)/s*u,this.velTy=this.velTy*(1-u)+(this.ty.goal-l)/s*u,this.velTz=this.velTz*(1-u)+(this.tz.goal-c)/s*u};return{dragStart:()=>{this.velTheta=this.velPhi=this.velTx=this.velTy=this.velTz=0,this.lastMoveAt=performance.now()},primary:(t,i)=>e(()=>this.orbitBy(t,i)),pan:(t,i)=>e(()=>this.panBy(t,i)),zoomAt:(t,i,s)=>this.zoomAt(t,i,s),twistAt:(t,i,s)=>this.twistAt(t,i,s),shiftScroll:(t,i)=>this.orbitBy(t,i),dragEnd:(t,i)=>{!i||this.reduced.matches||(t==="primary"?(this.theta.drift=this.velTheta*.6,this.phi.drift=this.velPhi*.6):(this.tx.drift=this.velTx*.6,this.ty.drift=this.velTy*.6,this.tz.drift=this.velTz*.6),this.callbacks.onChange())},active:t=>this.onGesture?.(t),coasting:()=>[this.theta,this.phi,this.tx,this.ty,this.tz].some(t=>Math.abs(t.drift)>.001),stop:()=>{for(let t of[this.theta,this.phi,this.logDist,this.tx,this.ty,this.tz])t.drift=0},tap:(t,i,s)=>this.callbacks.onClick(t,i,0,s),doubleTap:(t,i)=>this.callbacks.onDoubleClick(t,i),context:(t,i)=>this.callbacks.onContext(t,i),contextDismiss:()=>this.callbacks.onContextDismiss?.()}}dispose(){this.disposed=!0,this.input.dispose()}};var Ws={sky:[.3,.36,.48],horizon:[.19,.19,.21],ground:[.22,.21,.2],surround:1.15,lights:[{dir:[-.4,-.6,.7],color:[1,.96,.88],intensity:3.4},{dir:[.6,.3,.2],color:[.71,.8,1],intensity:.78},{dir:[.3,.8,.4],color:[1,1,1],intensity:.5}],exposure:1.3},h0=Math.atan2(-.95,1),rp={width:256,height:128};function d0(n){let{width:e,height:t}=rp,i=new Uint16Array(e*t*4),s=n(1);for(let r=0;r<t;r++){let o=Math.sin(((r+.5)/t-.5)*Math.PI),a=Math.abs(o),l=a*a*(3-2*a),c=o>=0?Ws.sky:Ws.ground;for(let u=0;u<e;u++){let d=(r*e+u)*4;for(let h=0;h<3;h++){let f=Ws.horizon[h];i[d+h]=n((f+(c[h]-f)*l)*Ws.surround)}i[d+3]=s}}return i}var f0=.05,Su=class extends Error{location;engineStack;thrown;constructor(e,t,i){super(e.location?`${e.message} (at ${e.location}, during ${i})`:e.message),this.name="EnginePanic",this.location=e.location,this.engineStack=e.stack,this.thrown=t}};function p0(n){if(n?.aborted){let e=new Error("aborted");throw e.name="AbortError",e}}var Xa="cadpole:engine";var op=[];var kC=!1;function zC(n,e){op.push({name:n,ms:e,at:ap()}),op.length>512&&op.shift()}function lp(n,e){if(!kC)return e();let t=ap();try{return e()}finally{let i=ap()-t;zC(n,i),WC(n,t,i)}}function ap(){return performance.now()}function WC(n,e,t){try{performance.measure(`cadpole:${n}`,{start:e,duration:t}),performance.getEntriesByType("measure").length>512&&performance.clearMeasures()}catch{}}var Ka=class extends Error{constructor(e){super(e),this.name="WorkerGone"}},vu=class n{spawn;kind="worker";buildId;worker;nextId=1;waiting=new Map;disposed=!1;constructor(e,t,i){this.spawn=i,this.buildId=t,this.worker=e,this.listen(e)}static async start(e,t=2e4){if(typeof Worker>"u")throw new Error("this browser has no Worker");let i=()=>new Worker(e,{type:"module",name:"cadpole-engine"}),s=i(),r=await new Promise((o,a)=>{let l=setTimeout(()=>{d(),s.terminate(),a(new Error(`the engine worker did not start within ${t} ms`))},t),c=h=>{let f=h.data;!f||f.__cadpole!==Xa||(f.ok&&f.op==="ready"?(d(),o(f.buildId)):!f.ok&&f.op==="init"&&(d(),s.terminate(),a(new Error(f.message))))},u=h=>{d(),s.terminate(),a(new Error(h.message||"the engine worker failed to load"))},d=()=>{clearTimeout(l),s.removeEventListener("message",c),s.removeEventListener("error",u)};s.addEventListener("message",c),s.addEventListener("error",u)});return new n(s,r,i)}listen(e){e.addEventListener("message",t=>{let i=t.data;if(!i||i.__cadpole!==Xa||i.ok&&i.op==="ready")return;let s=this.waiting.get(i.id);s&&(this.waiting.delete(i.id),s.resolve(i))}),e.addEventListener("error",t=>{let i=t.message||"the engine worker failed";this.failAll(new Ka(i))})}failAll(e){let t=[...this.waiting.values()];this.waiting.clear();for(let i of t)i.reject(e)}restart(e){let t=this.worker;if(this.worker=null,this.failAll(new Ka(e)),t?.terminate(),this.disposed)return;let i=this.spawn();this.worker=i,this.listen(i)}send(e,t,i){p0(i);let s=this.worker;return s?new Promise((r,o)=>{let a=()=>this.restart("cancelled"),l=()=>i?.removeEventListener("abort",a);i?.addEventListener("abort",a,{once:!0}),this.waiting.set(e.id,{operation:t,resolve:c=>{l(),r(c)},reject:c=>{l(),o(c)}}),s.postMessage(e)}):Promise.reject(new Error("the engine worker is not running"))}failure(e,t){if(e.panic)return this.restart("the engine panicked"),new Su(e.panic,e.message,t);let i=new Error(e.message);return e.stack&&(i.stack=e.stack),i}async evaluate(e,t){let i=performance.now(),s=e.tolerance??f0,r=await this.send({__cadpole:Xa,id:this.nextId++,op:"evaluate",req:{scripts:e.scripts,overrides:e.overrides,tolerance:s,deadlineMs:e.deadlineMs??0,preview:e.preview===!0}},"evaluate",t);if(!r.ok)throw this.failure(r,"evaluate");if(r.op!=="evaluate")throw new Error("the engine worker answered the wrong question");let o=lp("engine.parse",()=>JSON.parse(r.modelJson)),a=lp("engine.normalize",()=>vd(o)),l=[];return r.meshes.forEach((c,u)=>{c&&(l[u]=c)}),{model:a,meshes:l,tolerance:s,kind:this.kind,elapsedMs:performance.now()-i}}async apiDoc(e){let t=await this.send({__cadpole:Xa,id:this.nextId++,op:"apiDoc"},"apiDoc",e);if(!t.ok)throw this.failure(t,"api_doc");if(t.op!=="apiDoc")throw new Error("the engine worker answered the wrong question");let i=JSON.parse(t.apiDocJson);return{functions:Array.isArray(i.functions)?i.functions:[],types:Array.isArray(i.types)?i.types:[]}}retire(){this.disposed=!0}dispose(){this.disposed=!0;let e=this.worker;this.worker=null,this.failAll(new Ka("disposed")),e?.terminate()}};var si=Math.PI/180;function HC(){let{width:n,height:e}=rp,t=new ps(d0(i=>mo.toHalfFloat(i)),n,e,gn,Sn);return t.mapping=vs,t.colorSpace=Wi,t.minFilter=Bt,t.magFilter=Bt,t.needsUpdate=!0,t}var up=class{canvas=document.createElement("canvas");renderer;scene=new mr;camera=new Kt(24,1,.1,1e4);root=new Yn;rig=new Yn;tmpV=new L;ro=new ResizeObserver(()=>this.resize());bodyMat=new Sr({color:12174529,roughness:.5,metalness:.1,side:sn,polygonOffset:!0,polygonOffsetFactor:0,polygonOffsetUnits:2});edgeMat=new gs({color:725521,transparent:!0,opacity:.55});ray=new wo;controls;owned=[];pending=0;fit=100;radius=50;centre=new L;view={az:-30,el:20};host=null;grabbed=!1;armed=!1;hintEl=null;hintTimer=0;modelKey="";triangles=0;lost=!1;frames=0;constructor(){let e=this.canvas;e.className="cp3d-canvas",e.tabIndex=0,e.setAttribute("role","img"),e.setAttribute("aria-label","Interactive 3D model. Drag to orbit, arrow keys to turn, plus and minus to zoom."),this.renderer=new xc({canvas:e,antialias:!0,alpha:!0,powerPreference:"high-performance"}),this.renderer.setPixelRatio(Math.min(window.devicePixelRatio||1,2)),this.renderer.toneMapping=Tr,this.renderer.toneMappingExposure=Ws.exposure,this.renderer.setClearColor(0,0),this.scene.environment=HC(),this.scene.environmentRotation.set(-Math.PI/2,0,0);for(let t of Ws.lights){let i=new vr(new He().setRGB(t.color[0],t.color[1],t.color[2]),t.intensity);i.position.set(t.dir[0],t.dir[1],t.dir[2]),this.rig.add(i)}this.camera.up.set(0,0,1),this.scene.add(this.root,this.rig),e.addEventListener("webglcontextlost",t=>{t.preventDefault(),this.lost=!0;let i=this.host;this.unmount(),i?.dispatchEvent(new CustomEvent("cp3d-lost"))}),e.addEventListener("wheel",t=>{!this.grabbed&&!this.armed&&!t.ctrlKey&&(t.stopImmediatePropagation(),this.hint())},{passive:!1}),e.addEventListener("pointerdown",t=>{t.pointerType==="mouse"&&(this.armed=!0)}),e.addEventListener("pointerleave",t=>{t.pointerType==="mouse"&&(this.armed=!1)}),this.controls=new bu(this.camera,e,{onChange:()=>this.requestRender(),onClick:()=>{},onDoubleClick:(t,i)=>this.focusAt(t,i),onContext:()=>{}}),e.addEventListener("keydown",t=>{let i=t.key,s=this.controls.pose(),r=e.getBoundingClientRect();if(i==="ArrowLeft")this.controls.setPose({theta:s.theta+8*si});else if(i==="ArrowRight")this.controls.setPose({theta:s.theta-8*si});else if(i==="ArrowUp")this.controls.setPose({phi:s.phi-6*si});else if(i==="ArrowDown")this.controls.setPose({phi:s.phi+6*si});else if(i==="+"||i==="=")this.controls.zoomAt(r.width/2,r.height/2,.88);else if(i==="-")this.controls.zoomAt(r.width/2,r.height/2,1.14);else if(i==="0"||i==="f")this.frame(!0);else if(i==="Escape"){this.host?.dispatchEvent(new CustomEvent("cp3d-exit"));return}else return;t.preventDefault()})}hint(){let e=this.host;e&&(this.hintEl||(this.hintEl=document.createElement("div"),this.hintEl.className="cp3d-hint",this.hintEl.setAttribute("aria-hidden","true"),this.hintEl.textContent="Click the model to give it the wheel"),this.hintEl.parentElement!==e&&e.appendChild(this.hintEl),this.hintEl.classList.add("on"),clearTimeout(this.hintTimer),this.hintTimer=window.setTimeout(()=>this.hintEl?.classList.remove("on"),1600))}focusAt(e,t){let i=this.canvas.getBoundingClientRect();this.ray.setFromCamera(new ze(e/Math.max(1,i.width)*2-1,-(t/Math.max(1,i.height))*2+1),this.camera);let s=this.ray.intersectObject(this.root,!0).find(r=>r.object.isMesh);s?this.controls.focus(s.point.clone()):this.frame(!0)}frame(e){let t=this.view;this.controls.setBounds(this.centre,this.radius),this.controls.setPose({theta:(t.az-90)*si,phi:(90-t.el)*si,distance:this.fit*(t.zoom??1),target:this.centre.clone()},e)}viewerPose(){let e=this.controls.pose(),t=Math.tan(this.camera.fov*si/2)/Math.tan(40*si/2);return{...e,distance:e.distance*t}}eyeTarget(){let e=this.controls.pose().target,t=Math.tan(this.camera.fov*si/2)/Math.tan(40*si/2),i=this.camera.position;return{eye:[e.x+(i.x-e.x)*t,e.y+(i.y-e.y)*t,e.z+(i.z-e.z)*t],target:[e.x,e.y,e.z]}}cameraNow(){let e=this.camera.position;return{x:e.x,y:e.y,z:e.z,d:this.controls.currentDistance,frames:this.frames}}mount(e,t){this.host!==e&&(this.host?.dispatchEvent(new CustomEvent("cp3d-unmount")),this.ro.disconnect(),this.host=e,this.armed=!1,e.appendChild(this.canvas),this.ro.observe(e)),this.grabbed=t,this.canvas.style.touchAction=t?"none":"pan-y",this.resize()}unmount(){let e=this.host;this.ro.disconnect(),this.canvas.remove(),this.host=null,this.clear(),e?.dispatchEvent(new CustomEvent("cp3d-unmount"))}resize(){let e=this.host;if(!e)return;let t=Math.max(1,e.clientWidth),i=Math.max(1,e.clientHeight);this.renderer.setSize(t,i,!1),this.camera.aspect=t/i,this.camera.updateProjectionMatrix(),this.draw()}clear(){this.root.clear();for(let e of this.owned)e.dispose();this.owned=[],this.triangles=0,this.modelKey=""}blank(){this.clear(),this.draw()}setModel(e,t,i=""){this.clear(),this.modelKey=i;let s=new Map,r=u=>{let d=s.get(u);if(d)return d;let h=e.meshes[u];if(!h||h.indices.length===0)return null;let f=new dn;return f.setAttribute("position",new Xt(h.positions,3)),f.setAttribute("normal",new Xt(h.normals,3)),f.setIndex(new Xt(h.indices,1)),s.set(u,f),this.owned.push(f),f},o=0;for(let u of e.model.bodies)o+=u.visible?(e.meshes[u.mesh_index]?.indices.length??0)/3:0;let a=o<12e4,l=new Map;for(let u of e.model.bodies){if(!u.visible)continue;let d=r(u.mesh_index);if(!d)continue;let h=this.bodyMat,f=u.material;if(f){h=this.bodyMat.clone(),this.owned.push(h),f.color&&h.color.setRGB(f.color[0],f.color[1],f.color[2]),f.roughness!==null&&(h.roughness=f.roughness),f.metallic!==null&&(h.metalness=f.metallic);let m=f.opacity===null?1:f.opacity;m<.999&&(h.transparent=!0,h.opacity=Math.max(.05,m),h.depthWrite=!1)}let g=new fn(d,h),y=u.transform;if(y&&(g.position.set(y.translation.x,y.translation.y,y.translation.z),g.quaternion.set(y.rotation.x,y.rotation.y,y.rotation.z,y.rotation.w)),a){let m=l.get(u.mesh_index);m||(m=new vo(d,30),l.set(u.mesh_index,m),this.owned.push(m));let p=new yo(m,this.edgeMat);p.raycast=()=>{},g.add(p)}this.root.add(g)}this.triangles=o,this.root.updateMatrixWorld(!0);let c=new ci().setFromObject(this.root);if(!c.isEmpty()){let u=c.getBoundingSphere(new Kn);t&&(this.view=t,this.camera.fov=t.fov??24),this.centre.copy(u.center),this.radius=u.radius;let d=this.camera.fov*si,h=2*Math.atan(Math.tan(d/2)*Math.min(1,this.camera.aspect));this.fit=u.radius/Math.sin(Math.min(d,h)/2)*1.02,this.camera.updateProjectionMatrix(),t&&this.frame(!1)}this.draw()}requestRender(){this.pending||(this.pending=requestAnimationFrame(e=>{this.pending=0;let t=this.controls.update(e);this.draw(!0),t&&this.requestRender()}))}draw(e=!1){if(!this.host||this.lost)return;e||this.controls.update(performance.now());let t=this.controls.currentDistance,i=this.camera,s=i.position.distanceTo(this.centre),r=s+this.radius*2;i.near=Math.max(.01,Math.min(this.radius/100,t/50),(s-this.radius)*.5),i.far=Math.max(r,t*2)*1.5,i.updateProjectionMatrix(),i.getWorldDirection(this.tmpV),Math.hypot(this.tmpV.x,this.tmpV.y)<.001&&this.tmpV.set(0,0,1).applyQuaternion(i.quaternion).negate(),this.rig.rotation.z=Math.atan2(-this.tmpV.y,-this.tmpV.x)-h0,this.renderer.render(this.scene,i),this.frames++}},xt=null;function $U(){try{let n=document.createElement("canvas");return!!(n.getContext("webgl2")||n.getContext("webgl"))}catch{return!1}}function y0(){return(!xt||xt.lost)&&(xt=new up),xt}var Jr=null,YC=[],m0=new Set,_0=new Set,ZC=performance.now();function Si(n){let e=`${((performance.now()-ZC)/1e3).toFixed(2)}s ${n}`;YC.push(e);for(let t of _0)t(e)}function JU(n){_0.add(n)}var cp=n=>`${(n/1048576).toFixed(1)} MB`,dp=[];document.addEventListener("securitypolicyviolation",n=>{let e=`${n.effectiveDirective||n.violatedDirective} blocked ${n.blockedURI||"inline code"}`;dp.push(e),Si(`Content-Security-Policy: ${e} (policy: ${n.originalPolicy})`)});var $a=null,hp=[];function qU(){return new Promise(n=>{if(!$a)return n(0);hp.push(n),$a.postMessage({ask:"memory"}),setTimeout(()=>n(0),2e3)})}function XC(){if(typeof WebAssembly!="object"||typeof WebAssembly.instantiate!="function")return"WebAssembly is switched off in this browser. On an iPhone or iPad, Lockdown Mode does that.";if(typeof Worker>"u")return"This browser has no web workers.";try{new WebAssembly.Module(Uint8Array.of(0,97,115,109,1,0,0,0))}catch(e){let t=e instanceof Error?e.message:String(e);return Si(`WebAssembly refused a trivial module: ${t}`),dp.length>0||/Content Security Policy|unsafe-eval|CSP/i.test(t)?"This address is served with a Content-Security-Policy that blocks WebAssembly (script-src lacks 'wasm-unsafe-eval'), so the engine cannot run from here. It is the server's setting, not your device.":`This browser refused to compile WebAssembly: ${t}`}let n=Uint8Array.of(0,97,115,109,1,0,0,0,1,5,1,96,0,1,123,3,2,1,0,10,10,1,8,0,65,0,253,15,253,98,11);return WebAssembly.validate(n)?null:"This browser is too old for the engine: it needs WebAssembly SIMD (iOS 16.4 or later, Chrome 91, Firefox 89)."}function QU(n){let e=n instanceof Error?n.message:String(n),t=dp.find(i=>/worker-src|script-src/.test(i));return/Content Security Policy|Content-Security-Policy|unsafe-eval/i.test(e)||t?`This address is served with a Content-Security-Policy that blocks the engine (${t??"WebAssembly compilation"}). It is the server's setting, not your device.`:/out of memory|allocation failed|could not allocate/i.test(e)?"This device ran out of memory while loading the engine. Closing other tabs usually frees enough.":/did not start within/.test(e)?"The engine took too long to download on this connection. Try again on Wi-Fi.":/HTTP \d+|Failed to fetch|Load failed|NetworkError|Importing a module script failed/i.test(e)?`The engine could not be downloaded (${e}). Check the connection and try again.`:e}function KC(n){if(n&&m0.add(n),!Jr){let e=Number(document.documentElement.dataset.engineBytes)||0,t=new URL(`./engine/site-engine.js?n=${e}`,import.meta.url).href;Si(`starting the engine: ${navigator.userAgent}`),Si(`device memory ${navigator.deviceMemory??"unknown"} GB, ${navigator.hardwareConcurrency??"?"} cores, secure context ${window.isSecureContext}`);let i=XC();if(i)Si(`cannot run: ${i}`),Jr=Promise.reject(new Error(i));else{if(!$a&&typeof BroadcastChannel=="function"){$a=new BroadcastChannel("cadpole-site-engine");let s=-1;$a.onmessage=r=>{let o=r.data;if(!o.ask){if(o.stage)Si(`worker: ${o.stage}${o.memory?` (wasm memory ${cp(o.memory)})`:""}`);else if(typeof o.memory=="number"){for(let a of hp)a(o.memory);hp=[]}if(typeof o.loaded=="number"&&typeof o.total=="number"){for(let l of m0)l({loaded:o.loaded,total:o.total});let a=Math.floor(10*o.loaded/Math.max(1,o.total));a!==s&&(s=a,Si(`worker: downloaded ${cp(o.loaded)} of ${cp(o.total)}`))}}}}Si(`worker: ${t}`),Jr=vu.start(t,18e4).then(s=>(Si(`ready: ${s.buildId}`),s))}Jr.catch(s=>{Si(`failed: ${s instanceof Error?`${s.name}: ${s.message}`:String(s)}`),Jr=null})}return Jr}async function $C(n){let e=await sx(n),t=new DataView(e.buffer,e.byteOffset,e.byteLength);if(new TextDecoder().decode(e.subarray(0,4))!=="CPM1")throw new Error("not a cpmesh file");let i=t.getUint32(4,!0),s=JSON.parse(new TextDecoder().decode(e.subarray(8,8+i))),r=8+i,o=l=>{let c=e.buffer.slice(e.byteOffset+r,e.byteOffset+r+l);return r+=l+(4-l%4)%4,c},a=s.meshes.map(l=>{let c=new Uint16Array(o(l.nv*6)),u=new Int8Array(o(l.nv*3)),d=l.wide?new Uint32Array(o(l.ni*4)):new Uint32Array(new Uint16Array(o(l.ni*2))),h=new Float32Array(l.nv*3),f=new Float32Array(l.nv*3);for(let g=0;g<h.length;g++){let y=g%3,m=l.lo[y];h[g]=m+(l.hi[y]-m)*c[g]/65535}for(let g=0;g<f.length;g+=3){let y=u[g],m=u[g+1],p=u[g+2],E=Math.hypot(y,m,p)||1;f[g]=y/E,f[g+1]=m/E,f[g+2]=p/E}return{positions:h,normals:f,indices:d}});return{model:{bodies:s.bodies},meshes:a}}var qr=new Map,ss=null,g0=0;async function jU(n,e,t,i=!0){ss=n;let s=++g0,r=qr.get(e);r||(r=fetch(e).then(l=>{if(!l.ok)throw new Error(`HTTP ${l.status}`);return l.arrayBuffer()}).then(l=>$C(new Uint8Array(l))),qr.set(e,r),r.catch(()=>qr.delete(e)),qr.size>2&&qr.delete(qr.keys().next().value));let o=await r;if(ss!==n||s!==g0)return{triangles:0,shown:!1};let a=y0();return a.mount(n,i),a.setModel(o,t,e),{triangles:a.triangles,shown:!0}}var x0=class{host;path;onResult;tolerance;busy=!1;next=null;epoch=0;key="";view;fresh=!0;constructor(e,t,i,s,r=.1){this.host=e,this.path=t,this.onResult=s,this.tolerance=r,this.view=i}show(e,t){this.epoch++,this.key=e,this.view=t,this.fresh=!0,this.next=null,xt?.host===this.host&&xt.blank()}run(e,t,i=!1){ss=this.host,this.next={source:e,overrides:{...t},preview:i,epoch:this.epoch},this.busy||this.pump()}async pump(){for(this.busy=!0;this.next;){let e=this.next;this.next=null;let t=performance.now(),i=null,s="";try{let c=await KC(),u=[{path:this.path,source:e.source}];i=await c.evaluate({scripts:u,overrides:e.overrides,tolerance:this.tolerance,deadlineMs:2e4,preview:e.preview})}catch(c){s=c instanceof Error?c.message:String(c)}if(e.epoch!==this.epoch)continue;if(!i){this.onResult({ok:!1,ms:performance.now()-t,model:null,error:s,triangles:0,key:this.key,drawn:!1});continue}let r=i.model.diagnostics.find(c=>c.severity==="error"),o=i.model.bodies.some(c=>c.visible),a=0,l=!1;if(o&&(ss===this.host||ss===null)){ss=this.host;let c=y0();c.host!==this.host&&(c.mount(this.host,!1),this.fresh=!0),c.setModel(i,this.fresh?this.view:null,this.key),this.fresh=!1,a=c.triangles,l=!0}this.onResult({ok:!r&&o,ms:i.model.eval_millis||performance.now()-t,model:i.model,error:r?r.message:o?"":"the script made no bodies",triangles:a,key:this.key,drawn:l})}this.busy=!1}};function eD(n){(!n||ss===n)&&(ss=null),xt&&(n&&xt.host!==n||xt.unmount())}function tD(n){xt?.host&&xt.mount(xt.host,n)}function nD(){xt?.canvas.focus({preventScroll:!0})}function iD(n){return!xt||!xt.host||n&&xt.host!==n?null:xt.viewerPose()}function sD(n){return!xt||!xt.host||n&&xt.host!==n?null:xt.eyeTarget()}function rD(){return xt?.host?xt.cameraNow():null}function oD(n){return xt?.host===n&&xt.modelKey?{key:xt.modelKey,triangles:xt.triangles}:null}export{x0 as Live,rD as cameraNow,eD as close,KC as engine,YC as engineLog,qU as engineMemory,QU as explainEngineFailure,sD as eyeTarget,nD as focusCanvas,JU as onEngineLog,jU as openModel,tD as setGrabbed,oD as shownIn,$U as supported,iD as viewerPose};
/*! Bundled license information:

three/build/three.core.js:
three/build/three.module.js:
three/build/three.webgpu.js:
  (**
   * @license
   * Copyright 2010-2026 Three.js Authors
   * SPDX-License-Identifier: MIT
   *)
*/
//# sourceMappingURL=site3d.js.map
