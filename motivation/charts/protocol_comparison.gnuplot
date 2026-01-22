set terminal png size 1200,800 font "sans,10" enhanced
set output 'charts/protocol_comparison.png'
set title 'Fáith: TCP vs QUIC (Google Target) (lower is better)'
set xlabel 'Number of Requests'
set ylabel 'Duration (ms)'
set grid ytics
set key outside right top
set style data histograms
set style histogram clustered gap 1
set style fill solid 0.8 border -1
set boxwidth 0.9
set offset 0,0,graph 0.15,0
set xtics ("1" 0, "10" 1, "100" 2)


plot 'charts/protocol_comparison_data.txt' using 2:xtic(1) title 'TCP', \
     '' using 3 title 'QUIC (Cubic)', \
     '' using 4 title 'QUIC (BBR)', \
     '' using ($0-0.27):2:(sprintf("%.0f",$2)) with labels center offset 0,1 font ",8" notitle, \
     '' using ($0):3:(sprintf("%.0f",$3)) with labels center offset 0,1 font ",8" notitle, \
     '' using ($0+0.27):4:(sprintf("%.0f",$4)) with labels center offset 0,1 font ",8" notitle
