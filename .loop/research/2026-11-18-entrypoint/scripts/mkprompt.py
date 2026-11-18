import random, sys, uuid
# args: nwords out_path ; vocabulary of pseudo-words, fresh nonce at the start
n = int(sys.argv[1]); rnd = random.Random()
syl = ["ka","lo","mi","ren","dus","tor","vel","ny","sha","pi","gor","ul","fen","ix","ba","qe","zo","wil","tra","ck"]
words = [''.join(rnd.choice(syl) for _ in range(rnd.randint(1, 3))) for _ in range(n)]
body = ' '.join(words)
nonce = uuid.uuid4().hex
open(sys.argv[2], 'w').write(f"NONCE {nonce}\n{body}\n\nIgnore the text above. Reply with exactly one word: OK")
