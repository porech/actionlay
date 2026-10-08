import { createFile } from 'mp4box';
function item(key, type, size, count, data) {
  const result = Buffer.alloc(8 + Math.ceil(data.length/4)*4);
  result.write(key); result[4] = type.charCodeAt(0); result[5] = size;
  result.writeUInt16BE(count,6); data.copy(result,8); return result;
}
function nested(key, children) { const bytes=Buffer.concat(children); return item(key,'\0',1,bytes.length,bytes); }
function ints(values) { const bytes=Buffer.alloc(values.length*4); values.forEach((v,i)=>bytes.writeInt32BE(v,i*4)); return bytes; }
export function gpmfFixture(count = 50) {
  const mp4 = createFile();
  const id=mp4.addTrack({type:'mett',hdlr:'meta',timescale:1000,duration:count*40,media_duration:count*40});
  const entry=mp4.getTrackById(id).mdia.minf.stbl.stsd.entries[0]; entry.type='gpmd'; entry.data=new Uint8Array([0,0]);
  for(let i=0;i<count;i++) {
    const gps=nested('DEVC',[nested('STRM',[
      item('GPSF','L',4,1,ints([3])), item('GPSU','U',16,1,Buffer.from('260101120000.000')),
      item('GPSP','S',2,1,Buffer.from([0,100])), item('SCAL','l',4,5,ints([10000000,10000000,1000,1000,100])),
      item('GPS5','l',20,1,ints([450000000+i*100,90000000,20000,5000+i*100,500])),
    ])]);
    mp4.addSample(id,gps,{dts:i*40,cts:i*40,duration:40,is_sync:true});
  }
  const buffer=Buffer.from(mp4.getBuffer().buffer); buffer.write('gpmd',buffer.indexOf('mett')); return buffer;
}
