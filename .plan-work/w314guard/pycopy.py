import os, re, shutil
root = r'C:\dev\CoresSystem\.claude\worktrees\epic-shaw-4d613b'
gen = os.path.join(root, '.plan-work', 'gen-py', 'model_plane', 'v1')
dst = os.path.join(root, 'apps', 'Model Plane', 'python', 'mp-events-py', 'src', 'mp_events', '_gen', 'model_plane', 'v1')


def norm(text):
    text = text.replace('\r\n', '\n')
    text = '\n'.join(l for l in text.split('\n') if not l.startswith('# Protobuf Python Version'))
    text = re.sub(r'_runtime_version\.ValidateProtobufRuntimeVersion\([^)]*\)\n?', '', text)
    return text


for name in sorted(os.listdir(gen)):
    src = os.path.join(gen, name)
    tgt = os.path.join(dst, name)
    if not os.path.isfile(tgt):
        continue
    if norm(open(src, encoding='utf-8').read()) != norm(open(tgt, encoding='utf-8').read()):
        print('DIFF', name)
        shutil.copyfile(src, tgt)
