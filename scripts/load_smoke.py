#!/usr/bin/env python3
import asyncio, sys, time, argparse
try:
    import aiohttp
except Exception:
    print('aiohttp missing, installing...')
    import subprocess
    subprocess.check_call([sys.executable, '-m', 'pip', 'install', 'aiohttp'])
    import aiohttp

async def worker(name, session, queue, results):
    while True:
        item = await queue.get()
        if item is None:
            queue.task_done(); break
        url, payload = item
        start = time.time()
        try:
            async with session.post(url, json=payload, timeout=30) as resp:
                text = await resp.text()
                status = resp.status
        except Exception as e:
            status = 0; text = str(e)
        latency = (time.time() - start) * 1000
        results.append((url, status, latency, text[:200]))
        queue.task_done()

async def run(endpoints, concurrency, duration):
    q = asyncio.Queue()
    results = []
    async with aiohttp.ClientSession() as session:
        workers = [asyncio.create_task(worker(f'w{i}', session, q, results)) for i in range(concurrency)]
        stop_at = time.time() + duration
        i=0
        while time.time() < stop_at:
            for ep in endpoints:
                url, payload = ep
                await q.put((url, payload))
                i+=1
        # drain
        await q.join()
        for _ in range(concurrency):
            await q.put(None)
        await asyncio.gather(*workers)
    return results

def summary(results):
    import statistics
    ok = [r for r in results if 200 <= r[1] < 300]
    errs = [r for r in results if not (200 <= r[1] < 300)]
    latencies = [r[2] for r in ok]
    print(f"Requests: {len(results)}  Successful: {len(ok)}  Errors: {len(errs)}")
    if latencies:
        print(f"Avg: {statistics.mean(latencies):.1f}ms  p50: {statistics.median(latencies):.1f}ms  p95: {statistics.quantiles(latencies, n=100)[94]:.1f}ms  p99: {statistics.quantiles(latencies, n=100)[98]:.1f}ms")
    if errs:
        print('\nSample errors:')
        for e in errs[:10]:
            print(f" {e[0]} -> status={e[1]} latency={e[2]:.1f}ms err={e[3]}")

if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('--concurrency', type=int, default=100)
    p.add_argument('--duration', type=int, default=30)
    p.add_argument('--base', type=str, default='http://localhost')
    args = p.parse_args()
    base = args.base
    # endpoints: retrieval and reasoning
    endpoints = [
        (f"{base}:8004/v1/retrieve", {"org_id":"smoke-test","query":"ocean covers earth","top_k":3}),
        (f"{base}:8101/api/v1/reason/batch", {"requests":[{"query":"What is 2 plus 2?","strategy":"chain_of_thought"}]})
    ]
    print(f"Running load smoke: concurrency={args.concurrency} duration={args.duration}s against {len(endpoints)} endpoints")
    res = asyncio.run(run(endpoints, args.concurrency, args.duration))
    summary(res)
