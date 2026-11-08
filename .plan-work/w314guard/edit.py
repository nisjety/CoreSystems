import sys
def edit(path, pairs):
    raw=open(path,'rb').read()
    crlf=raw.count(b'\r\n')>0 and raw.count(b'\r\n')==raw.count(b'\n')
    s=raw.decode('utf-8').replace('\r\n','\n')
    for a,b,n in pairs:
        c=s.count(a)
        assert c==n,(path,a[:80],c)
        s=s.replace(a,b)
    if crlf: s=s.replace('\n','\r\n')
    open(path,'wb').write(s.encode('utf-8'))
    print('ok',path,'crlf' if crlf else 'lf')
