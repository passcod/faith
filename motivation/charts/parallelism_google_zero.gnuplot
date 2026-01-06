set terminal png size 1200,800 font "sans,10" enhanced
set output 'charts/parallelism_google_zero.png'
set title 'Parallelism Impact: Google Target (100 requests) (Y-axis from zero)'
set xlabel 'Parallel Requests (SEQ)'
set ylabel 'Duration (ms)'
set yrange [0:*]
set grid ytics
set key outside right top
set style data histograms
set style histogram clustered gap 1
set style fill solid 0.8 border -1
set boxwidth 0.9
set offset 0,0,graph 0.15,0
set xtics ("1" 0, "10" 1, "25" 2, "50" 3)


plot 'charts/parallelism_google_data.txt' using 2:xtic(1) title 'native', \
     '' using 3 title 'node-fetch', \
     '' using 4 title 'Fáith-TCP', \
     '' using 5 title 'Fáith-QUIC-Cubic', \
     '' using 6 title 'Fáith-QUIC-BBR', \
     '' using ($0-0.4):2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle, \
     '' using ($0-0.2):3:(sprintf("%.0f",$3)) with labels center offset 0,1 font ",8" notitle, \
     '' using ($0):4:(sprintf("%.0f",$4)) with labels center offset 0,1 font ",8" notitle, \
     '' using ($0+0.2):5:(sprintf("%.0f",$5)) with labels center offset 0,1 font ",8" notitle, \
     '' using ($0+0.4):6:(sprintf("%.0f",$6)) with labels center offset 0,1 font ",8" notitle
