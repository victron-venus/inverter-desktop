"""Fetch and extract only the reviewed published RC package; never install it."""
import bz2
import gzip
import hashlib
import lzma
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import signal
import shutil
import sys
import tarfile
import urllib.request

REPOSITORY = 'victron-venus/inverter-desktop'
MAX_PACKAGE = 32 * 1024**2
MAX_EXPANDED = 256 * 1024**2


def require(value, message):
    if not value:
        raise RuntimeError(message)


class PublicAssetRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        from urllib.parse import urlsplit
        parsed = urlsplit(newurl)
        require(parsed.scheme == 'https' and parsed.hostname in
                ('release-assets.githubusercontent.com', 'objects.githubusercontent.com'),
                'Unexpected asset redirect')
        request = super().redirect_request(req, fp, code, msg, headers, newurl)
        request.remove_header('Authorization')
        return request


def get(path, limit, binary=False):
    require(path.startswith('repos/' + REPOSITORY + '/'), 'Repository route mismatch')
    headers = {'Accept': 'application/octet-stream' if binary else 'application/vnd.github+json',
               'X-GitHub-Api-Version': '2022-11-28', 'User-Agent': 'desktop-rc-smoke'}
    if os.environ.get('GH_TOKEN'):
        headers['Authorization'] = 'Bearer ' + os.environ['GH_TOKEN']
    request = urllib.request.Request('https://api.github.com/' + path, headers=headers)
    opener = urllib.request.build_opener(PublicAssetRedirect())
    with opener.open(request, timeout=20) as response:
        data = response.read(limit + 1)
    require(len(data) <= limit, 'HTTP byte cap exceeded')
    return data if binary else json.loads(data)


def data_archive(deb):
    require(deb.startswith(b'!<arch>\n'), 'Not a Debian ar archive')
    offset, members, data = 8, set(), None
    while offset < len(deb):
        header = deb[offset:offset + 60]
        require(len(header) == 60 and header[58:] == b'`\n', 'Invalid ar header')
        name = header[:16].decode('ascii').rstrip().rstrip('/')
        require(name not in members and len(members) < 8, 'Duplicate or excessive ar members')
        members.add(name)
        field = header[48:58].decode('ascii').strip()
        require(field.isdigit(), 'Invalid ar member size')
        size = int(field)
        offset += 60
        require(size <= MAX_PACKAGE and offset + size <= len(deb), 'Truncated ar member')
        payload = deb[offset:offset + size]
        if name == 'debian-binary':
            require(payload == b'2.0\n', 'Debian format mismatch')
        elif name in ('data.tar.xz', 'data.tar.gz', 'data.tar.bz2', 'data.tar'):
            require(data is None, 'Multiple data archives')
            data = (name, payload)
        else:
            require(name in ('control.tar.xz', 'control.tar.gz', 'control.tar.bz2', 'control.tar'),
                    'Unexpected ar member')
        offset += size + size % 2
    require(offset == len(deb) and 'debian-binary' in members and data is not None,
            'Incomplete Debian archive')
    return data


def bounded_tar(data):
    name, payload = data
    if name == 'data.tar.xz':
        decoder = lzma.LZMADecompressor(format=lzma.FORMAT_XZ, memlimit=64 * 1024**2)
        raw = decoder.decompress(payload, max_length=MAX_EXPANDED + 1)
        require(len(raw) <= MAX_EXPANDED, 'Expanded archive cap including metadata')
        require(decoder.eof and not decoder.unused_data, 'Incomplete or concatenated XZ archive')
    elif name in ('data.tar.gz', 'data.tar.bz2'):
        stream = (gzip.GzipFile(fileobj=io.BytesIO(payload), mode='rb') if name.endswith('.gz')
                  else bz2.BZ2File(io.BytesIO(payload), 'rb'))
        with stream as source:
            raw = source.read(MAX_EXPANDED + 1)
    else:
        raw = payload
    require(len(raw) <= MAX_EXPANDED, 'Expanded archive cap including metadata')
    validate_tar_end(raw)
    return raw


def validate_tar_end(raw):
    require(len(raw) % 512 == 0, 'Misaligned tar archive')
    offset, records = 0, 0
    while offset + 512 <= len(raw):
        header = raw[offset:offset + 512]
        if header == bytes(512):
            require(raw[offset + 512:offset + 1024] == bytes(512), 'Missing second tar end marker')
            require(not any(raw[offset + 1024:]), 'Non-padding tar tail')
            return
        field = header[124:136].strip(b'\0 ')
        require(re.fullmatch(rb'[0-7]+', field) is not None, 'Unsupported tar size encoding')
        size = int(field, 8)
        offset += 512 + ((size + 511) // 512) * 512
        records += 1
        require(offset <= len(raw) and records <= 8192, 'Truncated or excessive tar records')
    raise RuntimeError('Missing tar end markers')


def extract(deb, destination):
    destination.mkdir(exist_ok=False)
    seen, total = set(), 0
    with tarfile.open(fileobj=io.BytesIO(bounded_tar(data_archive(deb))), mode='r:') as archive:
        for member in archive:
            name = member.name
            while name.startswith('./'):
                name = name[2:]
            if name in ('', '.') and member.isdir():
                continue
            path = PurePosixPath(name)
            require(not path.is_absolute() and '..' not in path.parts and name == path.as_posix(),
                    'Unsafe package path')
            require(name not in seen and len(seen) < 4096, 'Duplicate or excessive package entries')
            seen.add(name)
            require(member.isdir() or member.isfile(), 'Links and special package entries are not allowed')
            require(member.mode in (0o644, 0o755), 'Unexpected package permissions')
            target = destination.joinpath(*path.parts)
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                total += member.size
                require(0 <= member.size <= MAX_EXPANDED and total <= MAX_EXPANDED, 'Expanded package cap')
                target.parent.mkdir(parents=True, exist_ok=True)
                with archive.extractfile(member) as source, target.open('xb') as output:
                    remaining = member.size
                    while remaining:
                        chunk = source.read(min(1024 * 1024, remaining))
                        require(chunk, 'Truncated tar payload')
                        output.write(chunk)
                        remaining -= len(chunk)
                target.chmod(member.mode)
    executable = destination / 'usr/bin/inverter-dashboard'
    require(executable.is_file() and executable.stat().st_mode & 0o111, 'Package executable missing')
    return hashlib.sha256(executable.read_bytes()).hexdigest(), len(seen), total


def main():
    require(sys.platform == 'linux', 'Artifact smoke preparation is hosted Linux only')
    binding = json.loads(Path(sys.argv[1]).read_text())
    require(binding['activation'] == 'ACCEPTED_RC' and binding['repository'] == REPOSITORY,
            'Reviewed RC binding is not active')
    require(re.fullmatch(r'[0-9a-f]{40}', binding['source']) and
            re.fullmatch(r'v\d+\.\d+\.\d+-rc\.\d+', binding['tag']), 'Invalid source/tag')
    require(0 < binding['size'] <= MAX_PACKAGE and re.fullmatch(r'[0-9a-f]{64}', binding['sha256']),
            'Invalid package bounds')
    root = Path(sys.argv[2]).resolve()
    root.mkdir(exist_ok=False, parents=True)
    prior = signal.getsignal(signal.SIGALRM)
    def deadline(_signal, _frame):
        raise TimeoutError('Original 120-second preparation deadline')
    signal.signal(signal.SIGALRM, deadline)
    signal.setitimer(signal.ITIMER_REAL, 120)
    try:
        release = get(f'repos/{REPOSITORY}/releases/tags/' + binding['tag'], 2 * 1024**2)
        require(release['id'] == binding['release_id'] and not release['draft'] and release['prerelease']
                and release['target_commitish'] == binding['source'], 'Published RC identity changed')
        selected = [a for a in release['assets'] if a['id'] == binding['asset_id']]
        require(len(selected) == 1, 'Exact RC asset missing')
        asset = selected[0]
        require(asset['name'] == binding['name'] and asset['size'] == binding['size'] and
                asset['digest'] == 'sha256:' + binding['sha256'], 'Asset metadata changed')
        deb = get(f'repos/{REPOSITORY}/releases/assets/' + str(binding['asset_id']), binding['size'], True)
        require(len(deb) == binding['size'] and hashlib.sha256(deb).hexdigest() == binding['sha256'],
                'Published package byte mismatch')
        executable_hash, entries, expanded = extract(deb, root / 'package')
        binding.update(executable_sha256=executable_hash, extracted_entries=entries, expanded_bytes=expanded)
        (root / 'prepared-binding.json').write_text(json.dumps(binding, indent=2) + '\n')
    except BaseException:
        shutil.rmtree(root)
        raise
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, prior)


if __name__ == '__main__':
    main()
