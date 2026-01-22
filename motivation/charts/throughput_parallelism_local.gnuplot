set terminal png size 1200,800 font "sans,10" enhanced
set output 'charts/throughput_parallelism_local.png'
set title 'Throughput by Parallelism: Local Target (100 requests) (higher is better)'
set xlabel 'Parallel Requests (SEQ)'
set ylabel 'Requests/Second'
set grid ytics
set key outside right top
set style data histograms
set style histogram clustered gap 1
set style fill solid 0.8 border -1
set boxwidth 0.9
set offset 0,0,graph 0.15,0
set xtics ("1" 0, "10" 1, "25" 2, "50" 3)


plot 'charts/throughput_parallelism_local_data.txt' using 2:xtic(1) title 'native', \
     '' using 3 title 'node-fetch', \
     '' using 4 title 'Fáith', \
     '' using ($0-0.27):2:(sprintf("%.1f",$2)) with labels center offset 0,1 font ",8" notitle, \
     '' using ($0):3:(sprintf("%.1f",$3)) with labels center offset 0,1 font ",8" notitle, \
     '' using ($0+0.27):4:(sprintf("%.1f",$4)) with labels center offset 0,1 font ",8" notitle
