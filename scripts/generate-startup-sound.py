"""Original HITDROL frost cue. No samples; impact at 1.10 s matches intro.rs.
Run manually when changing the sound. Cargo uses the committed WAV.
"""
import array
import math
from pathlib import Path
import random
import sys
import wave

RATE, DURATION, IMPACT = 44100, 2.5, 1.10

def smooth(a, b, t):
    x = max(0.0, min(1.0, (t-a)/(b-a)))
    return x*x*(3-2*x)

def generate():
    rng = random.Random(0x48495444)
    dry, low = [], 0.0
    grains = [(IMPACT+.012+rng.random()*.42, 1500+rng.random()*2900,
               .009+rng.random()*.018, 25+rng.random()*50) for _ in range(22)]
    for n in range(round(RATE*DURATION)):
        t = n/RATE
        noise = rng.uniform(-1, 1)
        low += .07*(noise-low)
        air = noise-low
        rising = smooth(.30, 1.06, t)*(1-smooth(1.06, 1.14, t))
        value = low*rising*.22 + air*rising*.025
        value += math.sin(math.tau*174.61*t)*.014*smooth(0, .15, t)*(1-smooth(.35, .82, t))
        u = t-IMPACT
        if u >= 0:
            attack = 1-math.exp(-u*1800)
            value += .34*attack*math.exp(-u*26)*math.sin(math.tau*(88*u-18*u*u))
            value += .19*attack*math.exp(-u*48)*(air*.50+low)
            for k, hz in enumerate([698.46, 1046.50, 1396.91, 2093., 2793.83]):
                value += .075/(k+1)**.7*attack*math.exp(-u*(3.7+k*.5))*math.sin(math.tau*hz*u)
        for start, hz, gain, decay in grains:
            u = t-start
            if 0 <= u < .3:
                value += gain*(1-math.exp(-u*3000))*math.exp(-u*decay)*math.sin(math.tau*hz*u)
        dry.append(value)
    channels = []
    for n, value in enumerate(dry):
        t = n/RATE
        fade = smooth(0, .015, t)*(1-smooth(2.18, 2.47, t))
        echo_l = sum(dry[n-int(RATE*d)]*g for d,g in [(.071,.19),(.167,.10),(.257,.05)] if n >= int(RATE*d))
        echo_r = sum(dry[n-int(RATE*d)]*g for d,g in [(.093,.19),(.193,.10),(.283,.05)] if n >= int(RATE*d))
        channels.extend([(value+echo_l)*fade, (value+echo_r)*fade])
    peak = max(abs(x) for x in channels)
    pcm = array.array('h', (round(x/peak*.43*32767) for x in channels))
    if sys.byteorder != 'little': pcm.byteswap()
    destination = Path(__file__).resolve().parent.parent/'assets/intro/ice-startup.wav'
    with wave.open(str(destination), 'wb') as output:
        output.setnchannels(2)
        output.setsampwidth(2)
        output.setframerate(RATE)
        output.writeframes(pcm.tobytes())
    print(f'{destination}: {DURATION:.2f}s, stereo {RATE} Hz, peak -7.33 dBFS')

if __name__ == '__main__': generate()
